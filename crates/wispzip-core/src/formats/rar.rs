//! RAR extraction and listing via the vendored UnRAR library (read-only:
//! RAR compression is a proprietary algorithm). The library restores mtime
//! and permissions itself; we resolve every target path through the same
//! safety and conflict pipeline as the other formats.

use std::fs::{self};
use std::path::Path;

use crate::error::{io_err, Result, WispZipError};
use crate::extract::{
    ensure_parent, normalize_entry_name, EntrySelection, ExtractOptions,
    ExtractStats, TargetAction,
};
use crate::formats::days_from_civil;
use crate::progress::{Progress, Shared};

// ---- extract ---------------------------------------------------------------

/// Entries are streamed to the exact paths we resolve; cancellation takes
/// effect between entries (the UnRAR lib owns the per-file stream).
pub(crate) fn extract<F>(
    archive: &Path,
    opt: &ExtractOptions,
    shared: &Shared<F>,
    stats: &mut ExtractStats,
) -> Result<()>
where
    F: FnMut(&Progress) -> bool,
{
    // pre-pass for the progress total and the encrypted flag (a wrong password
    // on encrypted content surfaces as a CRC error; see rar_err_ext)
    let (total, has_encrypted): (u64, bool) = {
        let listing = unrar::Archive::new(archive)
            .open_for_listing()
            .map_err(|e| rar_err(e, archive))?;
        listing
            .filter_map(|e| e.ok())
            .fold((0u64, false), |(size, enc), h| {
                (
                    size + if h.is_directory() { 0 } else { h.unpacked_size },
                    enc || h.is_encrypted(),
                )
            })
    };
    {
        let mut r = shared.borrow_mut();
        r.total = Some(total);
        r.report(true);
    }

    let mut sel = EntrySelection::new(&opt.entries);
    let pw = opt.password.clone();
    let proc = match pw.as_deref() {
        Some(p) => unrar::Archive::with_password(archive, p),
        None => unrar::Archive::new(archive),
    }
    .open_for_processing()
    .map_err(|e| rar_err(e, archive))?;

    let mut proc = proc;
    while let Some(header) = proc.read_header().map_err(|e| rar_err(e, archive))? {
        shared.borrow().check_cancel()?;
        // copy what we need before extract_to consumes the header
        let (raw_name, size, is_dir) = {
            let e = header.entry();
            (
                e.filename.to_string_lossy().into_owned(),
                e.unpacked_size,
                e.is_directory(),
            )
        };
        let name = normalize_entry_name(&raw_name);
        if name.is_empty() || !sel.selects(&name) {
            proc = header.skip().map_err(|e| rar_err(e, archive))?;
            continue;
        }
        if is_dir {
            let target = opt.join_name(&name)?;
            fs::create_dir_all(&target).map_err(|e| io_err(e, &target))?;
            stats.dirs += 1;
            if sel.settled(&name) { break; }
            proc = header.skip().map_err(|e| rar_err(e, archive))?;
            continue;
        }
        {
            let mut r = shared.borrow_mut();
            r.current = Some(name.clone());
            r.report(true);
        }
        let target = match opt.resolve(&name)? {
            TargetAction::Write(t) => t,
            TargetAction::Skip => {
                stats.skipped += 1;
                proc = header.skip().map_err(|e| rar_err(e, archive))?;
                continue;
            }
        };
        ensure_parent(&target)?;
        // UnRAR refuses to overwrite; align with the Overwrite policy ourselves
        if target.symlink_metadata().is_ok() {
            fs::remove_file(&target).map_err(|e| io_err(e, &target))?;
        }
        proc = header
            .extract_to(&target)
            .map_err(|e| rar_err_ext(e, archive, has_encrypted))?;
        stats.files += 1;
        stats.bytes += size;
        {
            let mut r = shared.borrow_mut();
            r.files_done += 1;
            r.processed += size;
            r.report(true);
        }
        // All selected entries have been extracted: stop iterating the remaining entries
        if sel.settled(&name) { break; }
    }
    Ok(())
}

// ---- list ------------------------------------------------------------------

pub(crate) fn list(
    archive: &Path,
    password: Option<&str>,
) -> Result<Vec<super::EntryInfo>> {
    let ar = match password {
        Some(p) => unrar::Archive::with_password(archive, p),
        None => unrar::Archive::new(archive),
    };
    let entries = ar.open_for_listing().map_err(|e| rar_err(e, archive))?;
    let mut out = Vec::new();
    for entry in entries {
        let h = entry.map_err(|e| rar_err(e, archive))?;
        out.push(super::EntryInfo {
            is_dir: h.is_directory(),
            name: normalize_entry_name(&h.filename.to_string_lossy()),
            size: h.unpacked_size,
            // the UnRAR header does not expose packed size
            compressed_size: 0,
            mtime: dos_u32_to_unix(h.file_time),
            mode: None,
        });
    }
    Ok(out)
}

// ---- internals -------------------------------------------------------------

fn rar_err(e: unrar::error::UnrarError, archive: &Path) -> WispZipError {
    rar_err_ext(e, archive, false)
}

fn rar_err_ext(e: unrar::error::UnrarError, archive: &Path, encrypted: bool) -> WispZipError {
    use unrar::error::Code;
    match e.code {
        Code::MissingPassword | Code::BadPassword => {
            WispZipError::PasswordRequired(archive.display().to_string())
        }
        // AES-encrypted entries decoded with a wrong/missing password end in a
        // CRC failure; on plain archives it means real corruption.
        Code::BadData if encrypted => {
            WispZipError::PasswordRequired(archive.display().to_string())
        }
        _other => WispZipError::Other(format!("rar ({:?}): {e}", e.when)),
    }
}

/// RAR stores DOS-packed timestamps (year-1980 based).
fn dos_u32_to_unix(dos: u32) -> Option<i64> {
    let day = dos >> 16;
    let time = dos & 0xFFFF;
    let (y, m, d) = ((day >> 9) + 1980, (day >> 5) & 0xF, day & 0x1F);
    if m == 0 || d == 0 {
        return None;
    }
    let (hh, mm, ss) = (time >> 11, (time >> 5) & 0x3F, (time & 0x1F) * 2);
    Some(days_from_civil(y as i64, m, d) * 86_400 + (hh * 3600 + mm * 60 + ss) as i64)
}
