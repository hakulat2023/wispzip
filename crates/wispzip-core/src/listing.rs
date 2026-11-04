//! Read-only entry preview. Dispatches to the per-format `list` functions in
//! `formats/`; bare streams (gz/zst/xz/bz2) synthesize a single entry.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{io_err, Result};
use crate::extract::NameEncoding;
use crate::format::{detect, Classification, Format};
use crate::formats::streams;

/// Metadata for one entry inside an archive (read-only preview).
/// Field names mirror the frontend TS interface (EntryInfo in backend.ts): camelCase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInfo {
    pub name: String,
    pub is_dir: bool,
    /// Uncompressed size in bytes (0 when unknown, e.g. gz streams).
    pub size: u64,
    pub compressed_size: u64,
    /// Unix timestamp seconds, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    /// Unix permission bits, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

/// List the contents of `archive` without writing anything to disk.
///
/// `password` is only needed for 7z / rar archives with encrypted headers;
/// plain archives list without it.
pub fn list_archive(archive: &Path, encoding: NameEncoding, password: Option<&str>) -> Result<Vec<EntryInfo>> {
    // split volumes (<archive>.001 …): reassemble, then list normally
    if let Some(set) = crate::volumes::detect(archive)? {
        let tmp = crate::volumes::assemble(&set)?;
        let result = list_archive(&tmp, encoding, password);
        let _ = std::fs::remove_file(&tmp);
        return result;
    }

    let (format, class) = detect(archive)?;
    match class {
        Classification::SingleStream => {
            let meta = fs::metadata(archive).map_err(|e| io_err(e, archive))?;
            let mtime = meta.modified().ok().map(|t| t).and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
            });
            Ok(vec![EntryInfo {
                name: streams::output_name(archive),
                is_dir: false,
                size: 0,
                compressed_size: meta.len(),
                mtime,
                mode: None,
            }])
        }
        Classification::Archive => match format {
            Format::Zip => crate::formats::zip::list(archive, encoding),
            Format::SevenZ => crate::formats::sevenz::list(archive, password),
            Format::Rar => crate::formats::rar::list(archive, password),
            _ => crate::formats::tar::list(archive, format),
        },
    }
}
