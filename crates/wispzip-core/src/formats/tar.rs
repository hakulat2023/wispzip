//! tar and its compressed pipelines (tar.gz / tar.bz2 / tar.xz / tar.zst).
//! One tar builder, wrapped in the encoder selected by `Format`.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read};
use std::path::Path;

use crate::compress::{CompressOptions, PlanEntry};
use crate::error::{io_err, Result};
use crate::extract::{
    ensure_parent, normalize_entry_name, set_file_metadata, EntrySelection,
    ExtractOptions, ExtractStats, TargetAction,
};
use crate::format::Format;
use crate::progress::{system_time_from_unix, unix_secs, CountingReader, CountingWriter, Progress, Shared};

// ---- write -----------------------------------------------------------------

/// Compress into the tar container, wrapped in the encoder that `format`
/// selects. The plain `Format::Tar` arm just streams straight through.
pub(crate) fn write<F>(
    format: Format,
    entries: &[PlanEntry],
    sink: CountingWriter<BufWriter<File>, F>,
    opt: &CompressOptions,
    shared: &Shared<F>,
    tmp: &Path,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    let level = opt.level;
    match format {
        Format::Tar => {
            tar_build(entries, sink, shared)?;
        }
        Format::TarGz => {
            let enc = flate2::write::GzEncoder::new(sink, flate2::Compression::new(level.min(9) as u32));
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?;
        }
        Format::TarBz2 => {
            let enc = bzip2::write::BzEncoder::new(sink, bzip2::Compression::new(level.clamp(1, 9) as u32));
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?;
        }
        Format::TarXz => {
            let enc = xz2::write::XzEncoder::new(sink, level.min(9) as u32);
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?;
        }
        Format::TarZst => {
            let enc = zstd::stream::write::Encoder::new(sink, level.clamp(1, 22) as i32)
                .map_err(|e| io_err(e, tmp))?;
            let enc = tar_build(entries, enc, shared)?;
            enc.finish().map_err(|e| io_err(e, tmp))?;
        }
        _ => unreachable!("tar::write only handles tar-based formats"),
    }
    Ok(())
}

// ---- extract ---------------------------------------------------------------

pub(crate) fn extract<F>(
    archive: &Path,
    opt: &ExtractOptions,
    format: Format,
    shared: &Shared<F>,
    stats: &mut ExtractStats,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    let f = File::open(archive).map_err(|e| io_err(e, archive))?;
    let br = CountingReader::new(BufReader::new(f), shared.clone());
    match format {
        Format::Tar => tar_read(br, opt, shared, stats),
        Format::TarGz => tar_read(flate2::read::MultiGzDecoder::new(br), opt, shared, stats),
        Format::TarBz2 => tar_read(bzip2::read::BzDecoder::new(br), opt, shared, stats),
        Format::TarXz => tar_read(xz2::read::XzDecoder::new_multi_decoder(br), opt, shared, stats),
        Format::TarZst => {
            let dec = zstd::stream::read::Decoder::new(br).map_err(|e| io_err(e, archive))?;
            tar_read(dec, opt, shared, stats)
        }
        _ => unreachable!("tar::extract only handles tar-based formats"),
    }
}

// ---- list ------------------------------------------------------------------

pub(crate) fn list(
    archive: &Path,
    format: Format,
) -> Result<Vec<super::EntryInfo>> {
    let f = File::open(archive).map_err(|e| io_err(e, archive))?;
    let mut out = Vec::new();
    match format {
        Format::Tar => collect(tar::Archive::new(BufReader::new(f)), &mut out)?,
        Format::TarGz => collect(
            tar::Archive::new(flate2::read::MultiGzDecoder::new(BufReader::new(f))),
            &mut out,
        )?,
        Format::TarBz2 => collect(
            tar::Archive::new(bzip2::read::BzDecoder::new(BufReader::new(f))),
            &mut out,
        )?,
        Format::TarXz => collect(
            tar::Archive::new(xz2::read::XzDecoder::new_multi_decoder(BufReader::new(f))),
            &mut out,
        )?,
        Format::TarZst => {
            let dec =
                zstd::stream::read::Decoder::new(BufReader::new(f)).map_err(|e| io_err(e, archive))?;
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

fn tar_read<F, R>(r: R, opt: &ExtractOptions, shared: &Shared<F>, stats: &mut ExtractStats) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
    R: Read,
{
    let mut ar = tar::Archive::new(r);
    let mut sel = EntrySelection::new(&opt.entries);
    let entries = ar
        .entries()
        .map_err(|e| io_err(e, &opt.dest))?;
    for entry in entries {
        shared.borrow().check_cancel()?;
        let mut e = entry.map_err(|er| io_err(er, &opt.dest))?;
        let path = e
            .path()
            .map_err(|er| io_err(er, &opt.dest))?
            .to_path_buf();
        let name = normalize_entry_name(&path.to_string_lossy());
        // Stop as soon as the target entry is done: tar.gz does not need the second half of the stream
        if name.is_empty() || !sel.selects(&name) {
            continue;
        }
        let ty = e.header().entry_type();
        match ty {
            tar::EntryType::Directory => {
                let target = opt.join_name(&name)?;
                fs::create_dir_all(&target).map_err(|er| io_err(er, &target))?;
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
                let target = match opt.resolve(&name)? {
                    TargetAction::Write(t) => t,
                    TargetAction::Skip => {
                        stats.skipped += 1;
                        continue;
                    }
                };
                ensure_parent(&target)?;
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
                let h = e.header();
                let mtime = h.mtime().ok().map(system_time_from_unix);
                let mode = h.mode().ok().map(|m| m as u32);
                set_file_metadata(&file, mtime, mode);
                stats.files += 1;
                let mut r = shared.borrow_mut();
                r.files_done += 1;
                r.report(true);
            }
        }
        if sel.settled(&name) { break; }
    }
    Ok(())
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
