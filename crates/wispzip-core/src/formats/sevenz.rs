//! 7z via sevenz-rust: LZMA2 compression (optionally AES-256 encrypted
//! content), extraction and listing. File names stay in plain headers so
//! listing works without a password, matching 7-Zip defaults.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::compress::{CompressOptions, PlanEntry};
use crate::error::{cancel_io_error, io_err, Result, WispZipError, UNSAFE_ENTRY_PREFIX};
use crate::extract::{
    apply_dir_modes, normalize_entry_name, set_file_metadata, EntrySelection, ExtractOptions,
    ExtractStats, ParentDirs, TargetAction,
};
use crate::progress::{
    system_time_from_unix, unix_secs, CountingReader, CountingWriter, Progress, Shared,
};
use crate::source::ArchiveSource;

// ---- write -----------------------------------------------------------------

/// Compress with sevenz-rust's `SevenZWriter`. Content is LZMA2, optionally
/// wrapped in AES-256 (password); file names stay in plain headers so
/// `list_archive` keeps working without a password.
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
    use sevenz_rust::{
        AesEncoderOptions, MethodOptions, Password, SevenZArchiveEntry, SevenZMethod,
        SevenZMethodConfiguration, SevenZWriter,
    };

    let mut zw = SevenZWriter::new(sink).map_err(write_err)?;
    let lzma =
        SevenZMethodConfiguration::new(SevenZMethod::LZMA2).with_options(MethodOptions::LZMA2(
            sevenz_rust::lzma::LZMA2Options::with_preset(opt.level.min(9) as u32),
        ));
    match opt.password.as_deref().filter(|p| !p.is_empty()) {
        Some(pw) => {
            zw.set_encrypt_header(false);
            zw.set_content_methods(vec![
                SevenZMethodConfiguration::from(AesEncoderOptions::new(Password::from(pw))),
                lzma,
            ]);
        }
        None => {
            zw.set_content_methods(vec![lzma]);
        }
    }

    for e in entries {
        {
            let mut r = shared.borrow_mut();
            r.check_cancel()?;
            r.current = Some(e.name.clone());
            r.report(true);
        }
        let mut entry = SevenZArchiveEntry::new();
        entry.name = e.name.trim_end_matches('/').to_string();
        entry.is_directory = e.is_dir;
        entry.has_last_modified_date = true;
        entry.last_modified_date = filetime(unix_secs(e.mtime));
        if let Some(mode) = e.mode {
            entry.has_windows_attributes = true;
            entry.windows_attributes = unix_attributes(mode, e.is_dir);
        }
        if e.is_dir {
            zw.push_archive_entry(entry, None::<io::Empty>)
                .map_err(write_err)?;
        } else {
            let f = File::open(&e.src).map_err(|er| io_err(er, &e.src))?;
            let mut cr = CountingReader::new(BufReader::new(f), shared.clone());
            zw.push_archive_entry(entry, Some(&mut cr))
                .map_err(write_err)?;
            {
                let mut r = shared.borrow_mut();
                r.files_done += 1;
                r.report(true);
            }
        }
    }
    let sink = zw.finish().map_err(|e| io_err(e, Path::new("archive")))?;
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
    use sevenz_rust::SevenZReader;

    let had_pw = opt.password.is_some();
    let pw = match opt.password.as_deref() {
        Some(p) => sevenz_rust::Password::from(p),
        None => sevenz_rust::Password::empty(),
    };
    let archive_path = src.display_path();
    // SevenZReader::new takes the stream length explicitly, which is exactly
    // what a volume set reports — the 7z header at offset 0 is read through
    // the same seekable stream.
    let mut sz = SevenZReader::new(src.open()?, src.len(), pw)
        .map_err(|e| extract_err(e, &archive_path, had_pw))?;
    // Content encrypted with AES? Wrong/missing password then surfaces as a
    // checksum failure on the decoded stream, so remember it for the mapping.
    let content_encrypted = sz.archive().folders.iter().any(|f| {
        f.coders
            .iter()
            .any(|c| c.decompression_method_id() == sevenz_rust::SevenZMethod::ID_AES256SHA256)
    });
    let mut sel = EntrySelection::new(&opt.entries);
    let shared2 = shared.clone();
    let stats2 = std::rc::Rc::new(std::cell::RefCell::new(ExtractStats {
        files: 0,
        dirs: 0,
        bytes: 0,
        skipped: 0,
        dest: opt.dest.clone(),
    }));
    let st2 = stats2.clone();
    let opt2 = opt.clone();
    // Directory modes are applied once every entry is written (see apply_dir_modes).
    let dir_modes: std::rc::Rc<std::cell::RefCell<Vec<(PathBuf, u32)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let dm2 = dir_modes.clone();
    // Created directories are checked to resolve inside the destination, so a
    // pre-existing `sub -> /outside` symlink cannot be written through.
    let parents = std::rc::Rc::new(std::cell::RefCell::new(ParentDirs::new(&opt.dest)));
    let pd2 = parents.clone();

    sz.for_each_entries(move |entry, reader| {
        {
            let r = shared2.borrow();
            if r.cancelled() {
                return Err(sevenz_rust::Error::Io(
                    cancel_io_error(),
                    "cancelled".into(),
                ));
            }
        }
        let name = normalize_entry_name(&entry.name.clone());
        if name.is_empty() || !sel.selects(&name) {
            return Ok(true);
        }
        if entry.is_directory {
            let target = opt2.join_name(&name).map_err(wispzip_to_7z)?;
            pd2.borrow_mut().create(&target).map_err(wispzip_to_7z)?;
            if let Some(mode) = unix_mode(entry.windows_attributes) {
                dm2.borrow_mut().push((target, mode));
            }
            st2.borrow_mut().dirs += 1;
            return Ok(true);
        }
        // Empty files carry no stream (`has_stream == false`) but are not
        // directories: sevenz-rust already reads the kEmptyFile bitmap into
        // `is_directory`, so it is the only signal to trust here. Treating
        // "no stream" as "directory" turned every empty file packed by real
        // 7-Zip into an empty folder on extraction.
        if !entry.has_stream {
            let target = match opt2.resolve(&name).map_err(wispzip_to_7z)? {
                TargetAction::Write(t) => t,
                TargetAction::Skip => {
                    st2.borrow_mut().skipped += 1;
                    return Ok(true);
                }
            };
            pd2.borrow_mut().ensure(&target).map_err(wispzip_to_7z)?;
            let f = File::create(&target)
                .map_err(|e| sevenz_rust::Error::Io(e, format!("{target:?}").into()))?;
            set_file_metadata(&f, entry_mtime(entry), unix_mode(entry.windows_attributes));
            st2.borrow_mut().files += 1;
            {
                let mut r = shared2.borrow_mut();
                r.files_done += 1;
                r.report(false);
            }
            if sel.settled(&name) {
                return Ok(false);
            }
            return Ok(true);
        }
        let target = match opt2.resolve(&name).map_err(wispzip_to_7z)? {
            TargetAction::Write(t) => t,
            TargetAction::Skip => {
                st2.borrow_mut().skipped += 1;
                return Ok(true);
            }
        };
        pd2.borrow_mut().ensure(&target).map_err(wispzip_to_7z)?;
        let out = File::create(&target)
            .map_err(|e| sevenz_rust::Error::Io(e, format!("{target:?}").into()))?;
        let mut cw = CountingWriter::new(out, shared2.clone());
        let copied = io::copy(reader, &mut cw);
        let file = cw.into_inner();
        match copied {
            Ok(nb) => st2.borrow_mut().bytes += nb,
            Err(e) => {
                // On AES-encrypted content a read/decode failure is a wrong or
                // missing password (garbage stream) far more often than real
                // corruption; let extract_err report it as such.
                if content_encrypted && !crate::error::is_cancel_io(&e) {
                    return Err(sevenz_rust::Error::MaybeBadPassword(e));
                }
                return Err(sevenz_rust::Error::Io(
                    e,
                    format!("write {}", target.display()).into(),
                ));
            }
        }
        let mtime = entry_mtime(entry);
        set_file_metadata(&file, mtime, unix_mode(entry.windows_attributes));
        {
            let mut r = shared2.borrow_mut();
            r.files_done += 1;
            r.report(false);
        }
        st2.borrow_mut().files += 1;
        // All selected entries are done: stop decoding and skip the rest of the archive
        if sel.settled(&name) {
            return Ok(false);
        }
        Ok(true)
    })
    .map_err(|e| extract_err(e, &archive_path, content_encrypted))?;

    apply_dir_modes(&dir_modes.borrow());

    let s2 = stats2.borrow();
    stats.files += s2.files;
    stats.dirs += s2.dirs;
    stats.bytes += s2.bytes;
    stats.skipped += s2.skipped;
    Ok(())
}

// ---- list ------------------------------------------------------------------

pub(crate) fn list(src: &ArchiveSource, password: Option<&str>) -> Result<Vec<super::EntryInfo>> {
    use sevenz_rust::SevenZReader;

    let pw = match password {
        Some(p) => sevenz_rust::Password::from(p),
        None => sevenz_rust::Password::empty(),
    };
    let path = src.display_path();
    // SevenZReader (unlike Archive::open) decodes encrypted headers when a
    // password is given, so -mhe=on archives are listable too.
    let sz = SevenZReader::new(src.open()?, src.len(), pw).map_err(|e| list_err(e, &path))?;
    Ok(sz
        .archive()
        .files
        .iter()
        .map(|e| super::EntryInfo {
            is_dir: e.is_directory,
            name: normalize_entry_name(&e.name),
            size: e.size,
            compressed_size: e.compressed_size,
            mtime: if e.has_last_modified_date {
                Some(filetime_to_unix(u64::from(e.last_modified_date)))
            } else {
                None
            },
            mode: unix_mode(e.windows_attributes),
        })
        .collect())
}

// ---- metadata helpers ------------------------------------------------------

/// Entry modification time, or `None` when the archive does not carry one.
fn entry_mtime(entry: &sevenz_rust::SevenZArchiveEntry) -> Option<SystemTime> {
    if !entry.has_last_modified_date {
        return None;
    }
    let secs = filetime_to_unix(u64::from(entry.last_modified_date));
    (secs > 0).then(|| system_time_from_unix(secs as u64))
}

/// p7zip stores unix metadata in windows attributes when the unix-extension
/// flag (0x8000) is set: the high 16 bits hold st_mode including file-type
/// bits, the low 16 bits the DOS attributes. Verified against real 7-Zip.
pub(crate) fn unix_mode(attr: u32) -> Option<u32> {
    if attr & 0x8000 == 0 {
        return None;
    }
    Some(((attr >> 16) & 0xFFFF) & 0o7777)
}

/// Inverse of [unix_mode] for writing: (mode, is_dir) -> attributes.
fn unix_attributes(mode: u32, is_dir: bool) -> u32 {
    let st: u32 = if is_dir { 0o40000 } else { 0o100000 };
    (((st | mode) & 0xFFFF) << 16) | 0x8000 | if is_dir { 0x10 } else { 0x20 }
}

fn filetime(unix_secs: u64) -> sevenz_rust::nt_time::FileTime {
    sevenz_rust::nt_time::FileTime::from_unix_time(unix_secs as i64)
        .unwrap_or_else(|_| sevenz_rust::nt_time::FileTime::new(0))
}

/// Windows FILETIME (100ns ticks since 1601) -> unix seconds (0 on epoch 0).
pub(crate) fn filetime_to_unix(ticks: u64) -> i64 {
    const TICKS_PER_SEC: u64 = 10_000_000;
    const EPOCH_DIFF: u64 = 11_644_473_600;
    if ticks == 0 {
        return 0;
    }
    (ticks / TICKS_PER_SEC).saturating_sub(EPOCH_DIFF) as i64
}

// ---- error bridges ---------------------------------------------------------

fn write_err(e: sevenz_rust::Error) -> WispZipError {
    use sevenz_rust::Error as E;
    match e {
        E::Io(io, _) if crate::error::is_cancel_io(&io) => WispZipError::Cancelled,
        E::Io(io, ctx) => WispZipError::Other(format!("7z io error ({ctx}): {io}")),
        other => WispZipError::Other(format!("7z: {other}")),
    }
}

fn extract_err(e: sevenz_rust::Error, path: &Path, encrypted: bool) -> WispZipError {
    use sevenz_rust::Error as E;
    match e {
        E::Io(io, _) if crate::error::is_cancel_io(&io) => WispZipError::Cancelled,
        E::PasswordRequired => WispZipError::PasswordRequired(path.display().to_string()),
        // AES-encrypted content decoded with a wrong/missing password ends
        // here; on plain archives it means real corruption.
        E::ChecksumVerificationFailed | E::MaybeBadPassword(_) if encrypted => {
            WispZipError::PasswordRequired(path.display().to_string())
        }
        E::Io(io, ctx) => WispZipError::Other(format!("7z io error ({ctx}): {io}")),
        // ParentDirs rejects a directory that resolves outside the destination
        // with an UnsafeEntry; the closure could only carry it as `Other`, so
        // unwrap it here to keep the host-facing message (and variant) intact.
        E::Other(msg) => match msg.strip_prefix(UNSAFE_ENTRY_PREFIX) {
            Some(rest) => WispZipError::UnsafeEntry(rest.to_string()),
            None => WispZipError::Other(format!("7z: {msg}")),
        },
        other => WispZipError::Other(format!("7z: {other}")),
    }
}

/// Encrypted-header 7z archives fail header decode without the right
/// password; surface that as a password error, not a generic failure.
fn list_err(e: sevenz_rust::Error, archive: &Path) -> WispZipError {
    use sevenz_rust::Error as E;
    match e {
        E::PasswordRequired | E::ChecksumVerificationFailed | E::MaybeBadPassword(_) => {
            WispZipError::PasswordRequired(archive.display().to_string())
        }
        other => WispZipError::Other(format!("7z: {other}")),
    }
}

/// Bridge core errors into the sevenz closure's error type, preserving
/// cancellation so `extract_err` can map it back.
fn wispzip_to_7z(e: WispZipError) -> sevenz_rust::Error {
    match e {
        WispZipError::Cancelled => sevenz_rust::Error::Io(cancel_io_error(), "cancelled".into()),
        other => sevenz_rust::Error::Other(format!("{other}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion() {
        assert_eq!(filetime_to_unix(0), 0);
        // 2026-01-01T00:00:00Z = 1767225600
        let ticks = (1_767_225_600u64 + 11_644_473_600) * 10_000_000;
        assert_eq!(filetime_to_unix(ticks), 1_767_225_600);
    }

    #[test]
    fn unix_mode_roundtrip() {
        let attr = unix_attributes(0o754, false);
        assert_eq!(unix_mode(attr), Some(0o754));
        let attr = unix_attributes(0o700, true);
        assert_eq!(unix_mode(attr), Some(0o700));
        assert_eq!(unix_mode(0x20), None);
    }
}
