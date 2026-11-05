//! tar and its compressed pipelines (tar.gz / tar.bz2 / tar.xz / tar.zst).
//! One tar builder, wrapped in the encoder selected by `Format`.

use std::collections::{hash_map::DefaultHasher, HashMap};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, BufReader, BufWriter, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::SystemTime;

use crate::compress::{CompressOptions, PlanEntry};
use crate::error::{io_err, Result, WispZipError};
use crate::extract::{
    ensure_parent, normalize_entry_name, set_file_metadata, EntrySelection,
    ExtractOptions, ExtractStats, ParentDirs, TargetAction,
};
use crate::format::Format;
use crate::progress::{system_time_from_unix, unix_secs, CountingReader, CountingWriter, Progress, Shared};
use crate::source::ArchiveSource;

// ---- write -----------------------------------------------------------------

/// Compress into the tar container, wrapped in the encoder that `format`
/// selects. The plain `Format::Tar` arm just streams straight through.
pub(crate) fn write<F, S>(
    format: Format,
    entries: &[PlanEntry],
    sink: CountingWriter<S, F>,
    opt: &CompressOptions,
    shared: &Shared<F>,
    tmp: &Path,
) -> Result<CountingWriter<S, F>>
where
    F: FnMut(&Progress) -> bool,
    S: io::Write + io::Seek,
{
    let level = opt.level;
    let sink = match format {
        Format::Tar => tar_build(entries, sink, shared)?,
        Format::TarGz => {
            let enc = flate2::write::GzEncoder::new(sink, flate2::Compression::new(level.min(9) as u32));
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?
        }
        Format::TarBz2 => {
            let enc = bzip2::write::BzEncoder::new(sink, bzip2::Compression::new(level.clamp(1, 9) as u32));
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?
        }
        Format::TarXz => {
            let enc = xz2::write::XzEncoder::new(sink, level.min(9) as u32);
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?
        }
        Format::TarZst => {
            let enc = zstd::stream::write::Encoder::new(sink, level.clamp(1, 22) as i32)
                .map_err(|e| io_err(e, tmp))?;
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?
        }
        _ => unreachable!("tar::write only handles tar-based formats"),
    };
    Ok(sink)
}

// ---- extract ---------------------------------------------------------------

pub(crate) fn extract<F>(
    src: &ArchiveSource,
    opt: &ExtractOptions,
    format: Format,
    shared: &Shared<F>,
    stats: &mut ExtractStats,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    let path = src.display_path();
    let br = CountingReader::new(src.open()?, shared.clone());
    match format {
        Format::Tar => tar_read(br, opt, shared, stats),
        Format::TarGz => tar_read(flate2::read::MultiGzDecoder::new(br), opt, shared, stats),
        Format::TarBz2 => tar_read(bzip2::read::BzDecoder::new(br), opt, shared, stats),
        Format::TarXz => tar_read(xz2::read::XzDecoder::new_multi_decoder(br), opt, shared, stats),
        Format::TarZst => {
            let dec = zstd::stream::read::Decoder::new(br).map_err(|e| io_err(e, &path))?;
            tar_read(dec, opt, shared, stats)
        }
        _ => unreachable!("tar::extract only handles tar-based formats"),
    }
}

// ---- list ------------------------------------------------------------------

pub(crate) fn list(
    src: &ArchiveSource,
    format: Format,
) -> Result<Vec<super::EntryInfo>> {
    let path = src.display_path();
    let f = src.open()?;
    let mut out = Vec::new();
    match format {
        Format::Tar => collect(tar::Archive::new(f), &mut out)?,
        Format::TarGz => collect(
            tar::Archive::new(flate2::read::MultiGzDecoder::new(f)),
            &mut out,
        )?,
        Format::TarBz2 => collect(
            tar::Archive::new(bzip2::read::BzDecoder::new(f)),
            &mut out,
        )?,
        Format::TarXz => collect(
            tar::Archive::new(xz2::read::XzDecoder::new_multi_decoder(f)),
            &mut out,
        )?,
        Format::TarZst => {
            let dec = zstd::stream::read::Decoder::new(f).map_err(|e| io_err(e, &path))?;
            collect(tar::Archive::new(dec), &mut out)?
        }
        _ => unreachable!("tar::list only handles tar-based formats"),
    }
    Ok(out)
}

// ---- internals -------------------------------------------------------------

fn tar_build<F, W>(entries: &[PlanEntry], sink: W, shared: &Shared<F>) -> Result<W>
where
    F: FnMut(&Progress) -> bool,
    W: std::io::Write,
{
    let mut b = tar::Builder::new(sink);
    for e in entries {
        {
            let mut r = shared.borrow_mut();
            r.check_cancel()?;
            r.current = Some(e.name.clone());
            r.report(true);
        }
        let mut h = tar::Header::new_gnu();
        if e.is_dir {
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(e.mode.map(|m| m | 0o755).unwrap_or(0o755) & 0o7777);
        } else {
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(e.size);
            h.set_mode(e.mode.unwrap_or(0o644) & 0o7777);
        }
        h.set_mtime(unix_secs(e.mtime));
        h.set_cksum();
        if e.is_dir {
            b.append_data(&mut h, &e.name, io::empty())
                .map_err(|er| io_err(er, &e.src))?;
        } else {
            let f = File::open(&e.src).map_err(|er| io_err(er, &e.src))?;
            let mut cr = CountingReader::new(BufReader::new(f), shared.clone());
            b.append_data(&mut h, &e.name, &mut cr)
                .map_err(|er| io_err(er, &e.src))?;
            {
                let mut r = shared.borrow_mut();
                r.files_done += 1;
                r.report(true);
            }
        }
    }
    b.into_inner().map_err(|er| io_err(er, Path::new("archive")))
}

/// Entries at or below this size are decoded on the reader thread and handed
/// to a bounded writer pool. Larger entries stream straight to disk, so memory
/// stays O(1) regardless of archive size.
const SMALL_ENTRY_MAX: u64 = 64 * 1024;
/// Maximum number of small entries in flight across all writers.
const WRITE_QUEUE_CAP: usize = 64;

struct TarWork {
    target: PathBuf,
    data: Vec<u8>,
    mtime: Option<SystemTime>,
    mode: Option<u32>,
}

enum TarMsg {
    Done { target: PathBuf, bytes: u64 },
    Failed { target: PathBuf, error: WispZipError },
}

/// Writer count for tar-family extraction. gzip/bzip2/xz decoding is inherently
/// sequential; these workers only do file creation, writes and metadata, which
/// is where a small-file archive spends its time. Override with
/// `WISPZIP_TAR_WORKERS` (1 falls back to effectively serial writing).
fn tar_workers() -> usize {
    if let Ok(v) = std::env::var("WISPZIP_TAR_WORKERS") {
        if let Ok(n) = v.parse::<usize>() {
            return n.clamp(1, 8);
        }
    }
    // These writers are I/O bound (create / write / utime / chmod) while gzip
    // decoding stays on a single thread, so a 1-minute CPU load average is the
    // wrong signal: a busy build in the background would otherwise pin this to
    // one writer. Measured on a 13k-file source tarball, 4 writers is the
    // sweet spot and 8 is ~20% slower (metadata contention).
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(4)
        .max(1)
}

/// Entries sharing a target always go to the same writer (FIFO), preserving
/// conflict-policy order for duplicate names in an archive.
fn worker_slot(target: &Path, n: usize) -> usize {
    let mut h = DefaultHasher::new();
    target.as_os_str().hash(&mut h);
    (h.finish() as usize) % n
}

fn dec_inflight(inflight: &mut HashMap<PathBuf, usize>, target: &Path) {
    if let Some(c) = inflight.get_mut(target) {
        *c -= 1;
        if *c == 0 {
            inflight.remove(target);
        }
    }
}

fn apply_tar_msg<F>(
    msg: TarMsg,
    shared: &Shared<F>,
    stats: &mut ExtractStats,
    failure: &mut Option<WispZipError>,
    inflight: &mut HashMap<PathBuf, usize>,
) where
    F: FnMut(&Progress) -> bool,
{
    match msg {
        TarMsg::Done { target, bytes } => {
            dec_inflight(inflight, &target);
            stats.files += 1;
            stats.bytes += bytes;
            let mut r = shared.borrow_mut();
            r.output += bytes;
            r.files_done += 1;
            // Throttled: the old code forced one UI event per file, which for a
            // 13k-file tarball meant 13k webview round-trips.
            r.report(false);
        }
        TarMsg::Failed { target, error } => {
            dec_inflight(inflight, &target);
            if failure.is_none() {
                *failure = Some(error);
            }
        }
    }
}

fn tar_read<F, R>(r: R, opt: &ExtractOptions, shared: &Shared<F>, stats: &mut ExtractStats) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
    R: Read,
{
    let mut ar = tar::Archive::new(r);
    let mut sel = EntrySelection::new(&opt.entries);
    let mut parents = ParentDirs::new();
    let entries = ar
        .entries()
        .map_err(|e| io_err(e, &opt.dest))?;

    let n_workers = tar_workers();
    let stop = Arc::new(AtomicBool::new(false));
    let (result_tx, result_rx) = mpsc::channel::<TarMsg>();
    let mut senders: Vec<mpsc::SyncSender<TarWork>> = Vec::with_capacity(n_workers);
    let mut handles = Vec::with_capacity(n_workers);
    let cap = (WRITE_QUEUE_CAP / n_workers).max(4);
    for _ in 0..n_workers {
        let (tx, rx) = mpsc::sync_channel::<TarWork>(cap);
        let result_tx = result_tx.clone();
        let stop = Arc::clone(&stop);
        handles.push(thread::spawn(move || {
            for w in rx {
                if stop.load(Ordering::SeqCst) {
                    let _ = result_tx.send(TarMsg::Done { target: w.target, bytes: 0 });
                    continue;
                }
                let res = (|| -> Result<u64> {
                    let mut f = File::create(&w.target).map_err(|e| io_err(e, &w.target))?;
                    f.write_all(&w.data).map_err(|e| io_err(e, &w.target))?;
                    f.flush().map_err(|e| io_err(e, &w.target))?;
                    set_file_metadata(&f, w.mtime, w.mode);
                    Ok(w.data.len() as u64)
                })();
                let msg = match res {
                    Ok(bytes) => TarMsg::Done { target: w.target, bytes },
                    Err(error) => TarMsg::Failed { target: w.target, error },
                };
                let _ = result_tx.send(msg);
            }
        }));
        senders.push(tx);
    }
    drop(result_tx);

    let mut failure: Option<WispZipError> = None;
    let mut pending = 0usize;
    let mut inflight: HashMap<PathBuf, usize> = HashMap::new();

    let loop_result: Result<()> = (|| {
        'entries: for entry in entries {
            shared.borrow().check_cancel()?;
            // Drain completed writes before decoding the next entry: bounds the
            // queue and keeps progress/cancel responsive.
            while let Ok(msg) = result_rx.try_recv() {
                pending -= 1;
                apply_tar_msg(msg, shared, stats, &mut failure, &mut inflight);
            }
            if failure.is_some() {
                break;
            }
            if shared.borrow().cancelled() {
                stop.store(true, Ordering::SeqCst);
            }
            let mut e = entry.map_err(|er| io_err(er, &opt.dest))?;
            let path = e
                .path()
                .map_err(|er| io_err(er, &opt.dest))?
                .to_path_buf();
            let name = normalize_entry_name(&path.to_string_lossy());
            if name.is_empty() || !sel.selects(&name) {
                continue;
            }
            let ty = e.header().entry_type();
            match ty {
                tar::EntryType::Directory => {
                    let target = opt.join_name(&name)?;
                    parents.create(&target)?;
                    stats.dirs += 1;
                }
                tar::EntryType::Symlink => {
                    if !recreate_symlink(&mut e, &name, opt, stats)? {
                        stats.skipped += 1;
                    }
                }
                tar::EntryType::Link | tar::EntryType::Continuous => {
                    stats.skipped += 1;
                }
                _ => {
                    let h = e.header();
                    let size = h.size().unwrap_or(0);
                    let mtime = h.mtime().ok().map(system_time_from_unix);
                    let mode = h.mode().ok().map(|m| m as u32);
                    // Resolve on this thread in stream order so conflict policy
                    // keeps its original semantics. If an earlier entry with
                    // the same target is still queued, wait for it to land
                    // before resolving: lstat must see the file on disk.
                    loop {
                        let target = match opt.resolve(&name)? {
                            TargetAction::Write(t) => t,
                            TargetAction::Skip => {
                                stats.skipped += 1;
                                continue 'entries;
                            }
                        };
                        if inflight.get(&target).copied().unwrap_or(0) == 0 {
                            parents.ensure(&target)?;
                            if size <= SMALL_ENTRY_MAX {
                                let mut data = Vec::with_capacity(size as usize);
                                e.read_to_end(&mut data).map_err(|er| io_err(er, &target))?;
                                let work = TarWork { target: target.clone(), data, mtime, mode };
                                *inflight.entry(target.clone()).or_insert(0) += 1;
                                pending += 1;
                                let slot = worker_slot(&target, n_workers);
                                senders[slot].send(work).map_err(|_| {
                                    WispZipError::Other("tar writer pool stopped".into())
                                })?;
                            } else {
                                // Large entry: stream it here, never buffered in memory.
                                let out = File::create(&target).map_err(|er| io_err(er, &target))?;
                                let mut cw = CountingWriter::new(BufWriter::new(out), shared.clone());
                                let copied = io::copy(&mut e, &mut cw);
                                let file = match cw.into_inner().into_inner() {
                                    Ok(f) => f,
                                    Err(e) => return Err(io_err(e.into_error(), &target)),
                                };
                                match copied {
                                    Ok(nb) => stats.bytes += nb,
                                    Err(er) => return Err(io_err(er, &target)),
                                }
                                set_file_metadata(&file, mtime, mode);
                                stats.files += 1;
                                let mut r = shared.borrow_mut();
                                r.files_done += 1;
                                r.report(false);
                            }
                            break;
                        }
                        while inflight.get(&target).copied().unwrap_or(0) > 0 {
                            match result_rx.recv() {
                                Ok(msg) => {
                                    pending -= 1;
                                    apply_tar_msg(msg, shared, stats, &mut failure, &mut inflight);
                                }
                                Err(_) => break,
                            }
                        }
                        if failure.is_some() {
                            break;
                        }
                    }
                    if failure.is_some() {
                        break;
                    }
                }
            }
            if sel.settled(&name) { break; }
        }
        Ok(())
    })();

    // Always let queued writes finish so no worker outlives this call.
    drop(senders);
    for h in handles {
        let _ = h.join();
    }
    while pending > 0 {
        match result_rx.recv() {
            Ok(msg) => {
                pending -= 1;
                apply_tar_msg(msg, shared, stats, &mut failure, &mut inflight);
            }
            Err(_) => break,
        }
    }
    if let Err(e) = loop_result {
        if failure.is_none() {
            failure = Some(e);
        }
    }
    if shared.borrow().cancelled() {
        return Err(WispZipError::Cancelled);
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Recreate a symlink when the link target is relative; returns true on success.
#[cfg(unix)]
fn recreate_symlink<R: Read>(
    e: &mut tar::Entry<'_, R>,
    name: &str,
    opt: &ExtractOptions,
    stats: &mut ExtractStats,
) -> Result<bool> {
    use std::os::unix::fs::symlink;
    let link = match e.link_name() {
        Ok(Some(l)) => l.to_path_buf(),
        _ => return Ok(false),
    };
    if link.is_absolute() {
        return Ok(false);
    }
    let target = match opt.resolve(name)? {
        TargetAction::Write(t) => t,
        TargetAction::Skip => return Ok(false),
    };
    ensure_parent(&target)?;
    if target.symlink_metadata().is_ok() {
        let _ = fs::remove_file(&target);
    }
    match symlink(&link, &target) {
        Ok(()) => {
            stats.files += 1;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

#[cfg(not(unix))]
fn recreate_symlink<R: Read>(
    _e: &mut tar::Entry<'_, R>,
    _name: &str,
    _opt: &ExtractOptions,
    _stats: &mut ExtractStats,
) -> Result<bool> {
    Ok(false)
}

fn collect<R: std::io::Read>(mut ar: tar::Archive<R>, out: &mut Vec<super::EntryInfo>) -> Result<()> {
    let entries = ar
        .entries()
        .map_err(|e| crate::error::WispZipError::Other(format!("tar: {e}")))?;
    for entry in entries {
        let e = entry.map_err(|e| crate::error::WispZipError::Other(format!("tar: {e}")))?;
        let h = e.header();
        let path = e
            .path()
            .map_err(|er| crate::error::WispZipError::Other(format!("tar: {er}")))?
            .to_path_buf();
        let name = normalize_entry_name(&path.to_string_lossy());
        if name.is_empty() {
            continue;
        }
        out.push(super::EntryInfo {
            is_dir: h.entry_type().is_dir(),
            name,
            size: h.size().unwrap_or(0),
            compressed_size: h.size().unwrap_or(0),
            mtime: h.mtime().ok().map(|t| t as i64),
            mode: h.mode().ok().map(|m| m as u32),
        });
    }
    Ok(())
}
