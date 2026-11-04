//! Read-only entry preview. Dispatches to the per-format `list` functions in
//! `formats/`; bare streams (gz/zst/xz/bz2) synthesize a single entry.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{io_err, Result};
use crate::extract::NameEncoding;
use crate::format::{Classification, Format};
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
/// plain archives list without it. Split volumes are read as one stream, so a
/// listing never materialises the whole archive on disk.
pub fn list_archive(archive: &Path, encoding: NameEncoding, password: Option<&str>) -> Result<Vec<EntryInfo>> {
    let source = match crate::volumes::detect(archive)? {
        Some(set) => crate::source::ArchiveSource::from_volumes(set),
        None => crate::source::ArchiveSource::from_path(archive)?,
    };

    let (format, class) = crate::format::detect_source(&source)?;
    match class {
        Classification::SingleStream => {
            let meta = fs::metadata(source.display_path()).map_err(|e| io_err(e, archive))?;
            let mtime = meta.modified().ok().map(|t| t).and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
            });
            Ok(vec![EntryInfo {
                name: streams::output_name(&source.logical_path()),
                is_dir: false,
                size: 0,
                compressed_size: source.len(),
                mtime,
                mode: None,
            }])
        }
        Classification::Archive => match format {
            Format::Zip => crate::formats::zip::list(&source, encoding),
            Format::SevenZ => crate::formats::sevenz::list(&source, password),
            // UnRAR needs a real path; a split rar set is concatenated first.
            Format::Rar => match &source {
                crate::source::ArchiveSource::File { path, .. } => {
                    crate::formats::rar::list(path, password)
                }
                crate::source::ArchiveSource::Volumes(set) => {
                    let tmp = crate::volumes::assemble(set)?;
                    let result = crate::formats::rar::list(&tmp, password);
                    let _ = fs::remove_file(&tmp);
                    result
                }
            },
            _ => crate::formats::tar::list(&source, format),
        },
    }
}
