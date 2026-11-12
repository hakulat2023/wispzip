//! ZIP: write (Deflate / Store, AES-256, ZIP64), extract, list. Handles
//! UTF-8/GBK legacy entry-name decoding for both extract and list.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::compress::{CompressOptions, PlanEntry};
use crate::error::{io_err, Result, WispZipError};
use crate::extract::{
    apply_dir_modes, normalize_entry_name, set_file_metadata, EntrySelection, ExtractOptions,
    ExtractStats, NameEncoding, ParentDirs, TargetAction,
};
use crate::formats::days_from_civil;
use crate::progress::{system_time_from_unix, CountingReader, CountingWriter, Progress, Shared};
use crate::source::ArchiveSource;
use zip::{ZipArchive, ZipReadOptions};

// ---- write -----------------------------------------------------------------

pub(crate) fn write<F, S>(
    entries: &[PlanEntry],
    sink: CountingWriter<S, F>,
    opt: &CompressOptions,
    shared: &Shared<F>,
) -> Result<CountingWriter<S, F>>
where
    F: FnMut(&Progress) -> bool,
    S: io::Write + io::Seek,
{
    use zip::write::SimpleFileOptions;
    use zip::AesMode;
    use zip::CompressionMethod;
    use zip::ZipWriter;

    let mut zw = ZipWriter::new(sink);
    for e in entries {
        {
            let mut r = shared.borrow_mut();
            r.check_cancel()?;
            r.current = Some(e.name.clone());
            r.report(true);
        }
        let mut o = SimpleFileOptions::default()
            .compression_method(if opt.store_only {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            })
            .compression_level(if opt.store_only {
                None
            } else {
                Some(opt.level.min(9) as i64)
            })
            .large_file(true)
            .last_modified_time(dos_time(e.mtime)?);
        if let Some(mode) = e.mode {
            o = o.unix_permissions(mode);
        }
        if e.is_dir {
            zw.add_directory(&e.name, o)
                .map_err(|er| zip_err(er, &e.src))?;
        } else {
            let o = match opt.password.as_deref() {
                Some(pw) => o.with_aes_encryption(AesMode::Aes256, pw),
                None => o,
            };
            zw.start_file(&e.name, o)
                .map_err(|er| zip_err(er, &e.src))?;
            let f = File::open(&e.src).map_err(|er| io_err(er, &e.src))?;
            let mut cr = CountingReader::new(BufReader::new(f), shared.clone());
            io::copy(&mut cr, &mut zw).map_err(|er| io_err(er, &e.src))?;
            {
                let mut r = shared.borrow_mut();
                r.files_done += 1;
                r.report(true);
            }
        }
    }
    let sink = zw
        .finish()
        .map_err(|er| zip_err(er, Path::new("archive")))?;
    Ok(sink)
}

// ---- extract ---------------------------------------------------------------

pub(crate) fn extract<F>(
    src: &ArchiveSource,
    opt: &ExtractOptions,
    shared: &Shared<F>,
    stats: &mut ExtractStats,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    use std::io::Write as _;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;

    let path = src.display_path();
    let sel = EntrySelection::new(&opt.entries);
    let mut z = ZipArchive::new(src.open()?).map_err(|e| zip_err(e, &path))?;
    let n = z.len();

    // ---- Phase 1: zero-decompression scan. Target paths for selected entries are resolved serially
    // here (safe_join / conflict policy / parent dirs); worker threads only decompress and write.
    struct Work {
        index: usize,
        target: PathBuf,
        size: u64,
        mtime: Option<SystemTime>,
        mode: Option<u32>,
    }
    let mut work: Vec<Work> = Vec::new();
    let mut total: u64 = 0;
    // Every parent directory is created and verified here, on the main thread:
    // a pre-existing `sub -> /outside` symlink in the destination would
    // otherwise let the worker threads write through it.
    let mut parents = ParentDirs::new(&opt.dest);
    // Directory modes are recorded here and applied after the workers finish
    // (see apply_dir_modes) instead of at create time.
    let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();
    for i in 0..n {
        shared.borrow().check_cancel()?;
        let zf = z.by_index_raw(i).map_err(|e| zip_err(e, &path))?;
        let raw = zf.name_raw().to_vec();
        let name = decode_entry_name(&raw, opt.encoding);
        if name.is_empty() || !sel.selects(&name) {
            continue;
        }
        if zf.is_dir() {
            let target = opt.join_name(&name)?;
            parents.create(&target)?;
            if let Some(m) = zf.unix_mode() {
                dir_modes.push((target, m));
            }
            stats.dirs += 1;
            continue;
        }
        if zf.is_symlink() {
            stats.skipped += 1;
            continue;
        }
        let target = match opt.resolve(&name)? {
            TargetAction::Write(t) => t,
            TargetAction::Skip => {
                stats.skipped += 1;
                continue;
            }
        };
        parents.ensure(&target)?;
        total += zf.size();
        work.push(Work {
            index: i,
            target,
            size: zf.size(),
            mtime: zf
                .last_modified()
                .and_then(|dt| datetime_to_unix(&dt))
                .map(|s| system_time_from_unix(s as u64)),
            mode: zf.unix_mode(),
        });
    }
    shared.borrow_mut().total = Some(total);

    // ---- Phase 2: parallel extraction across a worker pool. Entries are independent; a cursor
    // dispatches large files first (longest task first, which shortens tail latency).
    work.sort_by_key(|w| std::cmp::Reverse(w.size));
    let work = Arc::new(work);
    let cursor = Arc::new(AtomicUsize::new(0));
    let workers_cancel = Arc::new(AtomicBool::new(false));
    let workers = crate::sysload::auto_workers(work.len(), total);
    let (tx, rx) = mpsc::sync_channel::<WorkerMsg>(64);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let work = Arc::clone(&work);
        let cursor = Arc::clone(&cursor);
        let cancel = Arc::clone(&workers_cancel);
        let tx = tx.clone();
        let src = src.clone();
        let password = opt.password.clone();
        handles.push(std::thread::spawn(move || {
            let fail = |e: WispZipError| {
                let _ = tx.send(WorkerMsg::Failed(e));
            };
            let reader = match src.open() {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            let mut z = match ZipArchive::new(reader) {
                Ok(z) => z,
                Err(e) => return fail(zip_err(e, &src.display_path())),
            };
            loop {
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let idx = cursor.fetch_add(1, Ordering::SeqCst);
                if idx >= work.len() {
                    return;
                }
                let item = &work[idx];
                let mut zf = match opt_password_open(&mut z, item.index, password.as_deref()) {
                    Ok(zf) => zf,
                    Err(e) => return fail(zip_err(e, &src.display_path())),
                };
                let out = match File::create(&item.target) {
                    Ok(o) => o,
                    Err(e) => {
                        return fail(WispZipError::Io {
                            path: item.target.clone(),
                            source: e,
                        })
                    }
                };
                let mut w = std::io::BufWriter::new(out);
                let mut buf = [0u8; 64 * 1024];
                let mut written = 0u64;
                loop {
                    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    match zf.read(&mut buf) {
                        Ok(0) => break,
                        Ok(nread) => {
                            if let Err(e) = w.write_all(&buf[..nread]) {
                                return fail(WispZipError::Io {
                                    path: item.target.clone(),
                                    source: e,
                                });
                            }
                            written += nread as u64;
                        }
                        Err(e) => {
                            // With a password supplied, a decrypt/CRC failure is almost always a wrong password
                            let err = if password.is_some() && !crate::error::is_cancel_io(&e) {
                                WispZipError::PasswordRequired(
                                    src.display_path().display().to_string(),
                                )
                            } else {
                                WispZipError::Io {
                                    path: item.target.clone(),
                                    source: e,
                                }
                            };
                            return fail(err);
                        }
                    }
                }
                let out_file = match w.into_inner() {
                    Ok(o) => o,
                    Err(e) => {
                        return fail(WispZipError::Io {
                            path: item.target.clone(),
                            source: e.into_error(),
                        })
                    }
                };
                set_file_metadata(&out_file, item.mtime, item.mode);
                let _ = tx.send(WorkerMsg::Done {
                    bytes: written,
                    name: std::path::Path::new(item.target.file_name().unwrap_or_default())
                        .to_string_lossy()
                        .into_owned(),
                });
            }
        }));
    }
    drop(tx);

    // ---- Main-thread aggregation: rate-limited progress/cancel/pause reporting (pause stalls the
    // aggregator; once the channel fills up, workers block on send, which is equivalent to pausing).
    let mut failed: Option<WispZipError> = None;
    let mut cancelled = false;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(80)) {
            Ok(WorkerMsg::Done { bytes, name, .. }) => {
                stats.files += 1;
                stats.bytes += bytes;
                let mut r = shared.borrow_mut();
                r.processed += bytes;
                r.files_done += 1;
                r.current = Some(name);
                r.report(false);
            }
            Ok(WorkerMsg::Failed(e)) => {
                if failed.is_none() {
                    failed = Some(e);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if shared.borrow().cancelled() {
                    workers_cancel.store(true, Ordering::SeqCst);
                    cancelled = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    for h in handles {
        let _ = h.join();
    }
    apply_dir_modes(&dir_modes);

    if cancelled || shared.borrow().cancelled() {
        return Err(WispZipError::Cancelled);
    }
    if let Some(e) = failed {
        // Failures take precedence over success stats (consistent with single-threaded behavior)
        return Err(e);
    }
    Ok(())
}

/// Pick read options depending on whether a password was supplied.
fn opt_password_open<'a, R: std::io::Read + std::io::Seek>(
    z: &'a mut ZipArchive<R>,
    index: usize,
    password: Option<&str>,
) -> zip::result::ZipResult<zip::read::ZipFile<'a, R>> {
    match password {
        Some(pw) => {
            z.by_index_with_options(index, ZipReadOptions::new().password(Some(pw.as_bytes())))
        }
        None => z.by_index(index),
    }
}

/// Message from a worker thread to the main-thread aggregator.
enum WorkerMsg {
    Done { bytes: u64, name: String },
    Failed(WispZipError),
}

// ---- list ------------------------------------------------------------------

pub(crate) fn list(src: &ArchiveSource, encoding: NameEncoding) -> Result<Vec<super::EntryInfo>> {
    use zip::ZipArchive;
    let path = src.display_path();
    let mut z = ZipArchive::new(src.open()?).map_err(|e| zip_err(e, &path))?;
    let mut out = Vec::with_capacity(z.len());
    for i in 0..z.len() {
        let zf = z.by_index_raw(i).map_err(|e| zip_err(e, &path))?;
        let name = decode_entry_name(zf.name_raw(), encoding);
        out.push(super::EntryInfo {
            is_dir: zf.is_dir(),
            name: normalize_entry_name(&name),
            size: zf.size(),
            compressed_size: zf.compressed_size(),
            mtime: zf.last_modified().and_then(|dt| datetime_to_unix(&dt)),
            mode: zf.unix_mode(),
        });
    }
    Ok(out)
}

// ---- helpers ---------------------------------------------------------------

fn zip_err(e: zip::result::ZipError, path: &Path) -> WispZipError {
    use zip::result::ZipError as Z;
    match e {
        Z::Io(io) => io_err(io, path),
        Z::InvalidPassword => WispZipError::PasswordRequired(path.display().to_string()),
        Z::UnsupportedArchive(Z::PASSWORD_REQUIRED) => {
            WispZipError::PasswordRequired(path.display().to_string())
        }
        other => WispZipError::Other(format!("zip: {other}")),
    }
}

/// SystemTime -> DOS timestamp with the 1980-2107 range clamped.
fn dos_time(t: SystemTime) -> Result<zip::DateTime> {
    let secs = crate::progress::unix_secs(t) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (mut y, m, d) = crate::formats::civil_from_days(days);
    y = y.clamp(1980, 2107);
    zip::DateTime::from_date_and_time(
        y as u16,
        m as u8,
        d as u8,
        (rem / 3600) as u8,
        ((rem % 3600) / 60) as u8,
        (rem % 60) as u8,
    )
    .map_err(|e| WispZipError::Other(format!("invalid timestamp: {e}")))
}

pub(super) fn datetime_to_unix(dt: &zip::DateTime) -> Option<i64> {
    let (y, m, d) = (dt.year() as i64, dt.month() as u32, dt.day() as u32);
    if !(1980..=2107).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(
        days_from_civil(y, m, d) * 86_400
            + dt.hour() as i64 * 3600
            + dt.minute() as i64 * 60
            + dt.second() as i64,
    )
}

/// Decode a raw zip entry name. Auto trusts valid UTF-8, then falls back to
/// GBK (the common encoding for Chinese Windows zip tools).
pub(crate) fn decode_entry_name(raw: &[u8], enc: NameEncoding) -> String {
    match enc {
        NameEncoding::Utf8 => String::from_utf8_lossy(raw).into_owned(),
        NameEncoding::Gbk => gbk_decode(raw),
        NameEncoding::Auto => String::from_utf8(raw.to_vec()).unwrap_or_else(|_| gbk_decode(raw)),
    }
}

fn gbk_decode(raw: &[u8]) -> String {
    let (s, _enc, _had_errors) = encoding_rs::GBK.decode(raw);
    s.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_time_clamps_before_1980() {
        let t = std::time::UNIX_EPOCH; // 1970-01-01
        let dt = dos_time(t).unwrap();
        assert_eq!(dt.year(), 1980);
    }

    #[test]
    fn decode_names_auto_prefers_utf8_then_gbk() {
        // "中文.txt" in GBK bytes: D6 D0 CE C4 2E 74 78 74
        let gbk_bytes = [0xD6, 0xD0, 0xCE, 0xC4, b'.', b't', b'x', b't'];
        assert_eq!(
            decode_entry_name(&gbk_bytes, NameEncoding::Auto),
            "中文.txt"
        );
        let utf8_bytes = "中文.txt".as_bytes();
        assert_eq!(
            decode_entry_name(utf8_bytes, NameEncoding::Auto),
            "中文.txt"
        );
    }

    #[test]
    fn backslash_entry_names_list_and_select() {
        // Some Windows tools use '\' as the entry separator. Both listing and name filtering
        // must unify to '/', otherwise extracting by name after selecting in the UI yields 0 files.
        use std::io::Write;

        use crate::{extract, list_archive, ExtractOptions};

        let dir = std::env::temp_dir().join(format!(
            "wispzip-bs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("bs.zip");

        let f = std::fs::File::create(&archive).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        zw.start_file("dir\\file.txt", o).unwrap();
        zw.write_all(b"hello").unwrap();
        zw.start_file("dir\\other.txt", o).unwrap();
        zw.write_all(b"world").unwrap();
        zw.finish().unwrap();

        // Listing outputs forward slashes
        let entries = list_archive(&archive, NameEncoding::Auto, None).unwrap();
        let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["dir/file.txt", "dir/other.txt"]);

        // Filtering by forward-slash names must extract and land in a normal directory layout
        let dest = dir.join("out");
        extract(
            &archive,
            &ExtractOptions {
                dest: dest.clone(),
                entries: Some(vec!["dir/file.txt".into()]),
                ..Default::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest.join("dir/file.txt")).unwrap(), b"hello");
        assert!(!dest.join("dir/other.txt").exists());
        assert_eq!(stats_files(&dest), 1);

        let _ = std::fs::remove_dir_all(&dir);

        // Counting helper: recursive file count
        fn stats_files(dir: &std::path::Path) -> usize {
            std::fs::read_dir(dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .map(|e| {
                            let p = e.path();
                            if p.is_dir() {
                                stats_files(&p)
                            } else {
                                1
                            }
                        })
                        .sum()
                })
                .unwrap_or(0)
        }
    }

    #[test]
    fn selective_extract_preserves_internal_layout() {
        // Extraction semantics regression: for whole-archive, folder-level and file-level selection,
        // in-archive relative paths (including a top-level same-name dir) are preserved under the
        // target dir; whether to wrap another same-name folder is decided by the caller (frontend option).
        // Regression background: selecting "files inside a subdirectory" used to extract 0 files.
        use std::io::Write;

        use crate::{extract, ExtractOptions};

        let dir = std::env::temp_dir().join(format!(
            "wispzip-layout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("layout.zip");

        // In-archive layout: Mango/a.docx, Mango/sub/b.xlsx, top.txt
        let f = std::fs::File::create(&archive).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        zw.start_file("Mango/a.docx", o).unwrap();
        zw.write_all(b"AAA").unwrap();
        zw.start_file("Mango/sub/b.xlsx", o).unwrap();
        zw.write_all(b"BBB").unwrap();
        zw.start_file("top.txt", o).unwrap();
        zw.write_all(b"TOP").unwrap();
        zw.finish().unwrap();

        // 1) Select a single file inside a subdirectory -> keep the in-archive relative path, extract only it
        let dest = dir.join("out-file");
        extract(
            &archive,
            &ExtractOptions {
                dest: dest.clone(),
                entries: Some(vec!["Mango/a.docx".into()]),
                ..Default::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest.join("Mango/a.docx")).unwrap(), b"AAA");
        assert!(!dest.join("top.txt").exists());
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1); // only the Mango/ level

        // 2) Folder-level quick extract (frontend expands to a file list, target = next to the archive)
        //    -> Mango/a.docx and Mango/sub/b.xlsx keep their layout
        let dest2 = dir.join("out-quick");
        extract(
            &archive,
            &ExtractOptions {
                dest: dest2.clone(),
                entries: Some(vec!["Mango/a.docx".into(), "Mango/sub/b.xlsx".into()]),
                ..Default::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest2.join("Mango/a.docx")).unwrap(), b"AAA");
        assert_eq!(
            std::fs::read(dest2.join("Mango/sub/b.xlsx")).unwrap(),
            b"BBB"
        );
        assert!(!dest2.join("top.txt").exists());

        // 3) Whole archive -> full layout
        let dest3 = dir.join("out-all");
        extract(
            &archive,
            &ExtractOptions {
                dest: dest3.clone(),
                ..Default::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest3.join("top.txt")).unwrap(), b"TOP");
        assert_eq!(
            std::fs::read(dest3.join("Mango/sub/b.xlsx")).unwrap(),
            b"BBB"
        );

        // 4) Extract with name conflicts -> auto-rename, never overwrite
        extract(
            &archive,
            &ExtractOptions {
                dest: dest.clone(),
                entries: Some(vec!["Mango/a.docx".into()]),
                ..Default::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest.join("Mango/a.docx")).unwrap(), b"AAA");
        let renamed = std::fs::read_dir(dest.join("Mango"))
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("a (1)"));
        assert!(
            renamed,
            "a conflicting file should be renamed to a (1).docx"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
