//! Compression orchestration: input planning, the per-format dispatch, and
//! byte-level split volumes. The format-specific writers live in `formats/`.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{io_err, Result, WispZipError};
use crate::format::Format;
use crate::progress::{
    new_cancel_flag, CountingWriter, Phase, Progress, Reporter, Shared,
};
use crate::volumes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressOptions {
    pub format: Format,
    /// Unified quality knob: 0-9 for zip/gz/xz/bz2/7z, 1-22 for zstd.
    #[serde(default = "default_level")]
    pub level: u8,
    /// Store entries without compression (zip only).
    #[serde(default)]
    pub store_only: bool,
    /// AES-256 encryption password (zip / 7z).
    #[serde(default)]
    pub password: Option<String>,
    /// After archiving, emit fixed-size volumes `<out>.001`, `<out>.002`, …
    /// instead of a single archive file (byte-level split, size in bytes).
    #[serde(default)]
    pub split: Option<u64>,
    /// Skip hidden entries (dotfiles like `.git`, `.DS_Store`) when planning.
    #[serde(default)]
    pub exclude_hidden: bool,
}

fn default_level() -> u8 {
    6
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            format: Format::Zip,
            level: default_level(),
            store_only: false,
            password: None,
            split: None,
            exclude_hidden: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressStats {
    pub input_files: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub skipped: u64,
    pub output_path: PathBuf,
}

/// One planned input: an on-disk source plus the archive name it will carry.
pub(crate) struct PlanEntry {
    pub(crate) src: PathBuf,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) is_dir: bool,
    pub(crate) mtime: SystemTime,
    pub(crate) mode: Option<u32>,
}

/// Compress `inputs` (files and/or directories) into `out`.
///
/// The output is written to a `.wispzip-part` sibling first and renamed into
/// place on success, so a failed or cancelled run never leaves a half-written
/// archive behind the final name.
pub fn compress<F>(inputs: &[PathBuf], out: &Path, opt: &CompressOptions, cb: F) -> Result<CompressStats>
where
    F: FnMut(&Progress) -> bool,
{
    if inputs.is_empty() {
        return Err(WispZipError::Other("no input files given".into()));
    }
    if matches!(
        opt.format,
        Format::Gz | Format::Zst | Format::Xz | Format::Bz2
    ) {
        if inputs.len() != 1 || inputs[0].is_dir() {
            return Err(WispZipError::UnsupportedFormat(
                "single-file streams (gz/zst/xz/bz2) compress exactly one file".into(),
            ));
        }
    }
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| io_err(e, parent))?;
        }
    }

    let (entries, input_bytes, files_total, skipped) = plan_opts(inputs, opt.exclude_hidden)?;
    let shared = Reporter::new(new_cancel_flag(), cb);
    {
        let mut r = shared.borrow_mut();
        r.phase = Phase::Compress;
        r.total = Some(input_bytes);
        r.files_total = Some(files_total);
        r.report(true);
    }

    let tmp = out.with_file_name(format!(
        "{}.wispzip-part",
        out.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into())
    ));

    let result = run_compress(&entries, &tmp, opt, &shared);
    match result {
        Ok(stats) => {
            let output_bytes = if let Some(vol) = opt.split.filter(|&v| v > 0) {
                split_into_volumes(&tmp, out, vol, &shared)?
            } else {
                if out.symlink_metadata().is_ok() {
                    fs::remove_file(out).map_err(|e| io_err(e, out))?;
                }
                fs::rename(&tmp, out).map_err(|e| io_err(e, out))?;
                stats
            };
            {
                let mut r = shared.borrow_mut();
                r.phase = Phase::Finish;
                r.report(true);
            }
            Ok(CompressStats {
                input_files: files_total,
                input_bytes,
                output_bytes,
                skipped,
                output_path: if opt.split.is_some() {
                    volumes::volume_path(out, 1)
                } else {
                    out.to_path_buf()
                },
            })
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Split the finished archive at `tmp` into `<out>.001`-style volumes and
/// remove `tmp`. Returns the total bytes written. Volumes are staged with a
/// `.wispzip-part` suffix and renamed into place, so an interrupted split never
/// leaves a truncated volume behind its final name.
fn split_into_volumes<F>(
    tmp: &Path,
    out: &Path,
    vol_size: u64,
    shared: &Shared<F>,
) -> Result<u64>
where
    F: FnMut(&Progress) -> bool,
{
    use std::io::Read;

    {
        let mut r = shared.borrow_mut();
        r.check_cancel()?;
        r.total = None;
        r.processed = 0;
        r.output = 0;
        r.report(true);
    }

    let mut src = BufReader::with_capacity(1 << 20, File::open(tmp).map_err(|e| io_err(e, tmp))?);
    let mut total = 0u64;
    let mut index = 1u32;
    loop {
        let vol_path = volumes::volume_path(out, index);
        let part_tmp =
            vol_path.with_file_name(format!("{}.wispzip-part", vol_path.file_name().unwrap_or_default().to_string_lossy()));
        let mut w = BufWriter::new(File::create(&part_tmp).map_err(|e| io_err(e, &part_tmp))?);
        let mut buf = vec![0u8; 1 << 20];
        let mut written = 0u64;
        let mut eof = false;
        while written < vol_size {
            let want = ((vol_size - written) as usize).min(buf.len());
            let n = src.read(&mut buf[..want]).map_err(|e| io_err(e, tmp))?;
            if n == 0 {
                eof = true;
                break;
            }
            w.write_all(&buf[..n]).map_err(|e| io_err(e, &part_tmp))?;
            written += n as u64;
            total += n as u64;
            {
                let mut r = shared.borrow_mut();
                r.output = total;
                r.check_cancel()?;
                r.report(false);
            }
        }
        w.flush().map_err(|e| io_err(e, &part_tmp))?;
        drop(w);
        if written == 0 {
            // exact multiple of vol_size: no empty trailing volume
            let _ = fs::remove_file(&part_tmp);
            break;
        }
        fs::rename(&part_tmp, &vol_path).map_err(|e| io_err(e, &vol_path))?;
        if eof {
            break;
        }
        index += 1;
    }

    fs::remove_file(tmp).map_err(|e| io_err(e, tmp))?;
    Ok(total)
}

fn run_compress<F>(
    entries: &[PlanEntry],
    tmp: &Path,
    opt: &CompressOptions,
    shared: &Shared<F>,
) -> Result<u64>
where
    F: FnMut(&Progress) -> bool,
{
    let file = File::create(tmp).map_err(|e| io_err(e, tmp))?;
    let sink = CountingWriter::new(BufWriter::new(file), shared.clone());
    let format = opt.format;
    match format {
        Format::Zip => crate::formats::zip::write(entries, sink, opt, shared),
        Format::Tar | Format::TarGz | Format::TarBz2 | Format::TarXz | Format::TarZst => {
            crate::formats::tar::write(format, entries, sink, opt, shared, tmp)
        }
        Format::Gz | Format::Zst | Format::Xz | Format::Bz2 => {
            crate::formats::streams::write(format, entries, sink, opt, shared)
        }
        Format::SevenZ => crate::formats::sevenz::write(entries, sink, opt, shared),
        Format::Rar => Err(WispZipError::UnsupportedFormat(
            "RAR compression is proprietary — extraction only".into(),
        )),
    }
    .map(|_| {
        shared.borrow().output
    })
}

// ---- planning ---------------------------------------------------------------

/// Summary of what a compress run would process, without writing anything.
/// Used by UIs to show "N files, X GB" on the confirmation screen.
pub fn scan(inputs: &[PathBuf], exclude_hidden: bool) -> Result<ScanStats> {
    let (entries, bytes, files, skipped) = plan_opts(inputs, exclude_hidden)?;
    Ok(ScanStats {
        files,
        dirs: entries.iter().filter(|e| e.is_dir).count() as u64,
        bytes,
        skipped,
        top_level: inputs.len() as u64,
    })
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub skipped: u64,
    pub top_level: u64,
}

fn plan_opts(
    inputs: &[PathBuf],
    exclude_hidden: bool,
) -> Result<(Vec<PlanEntry>, u64, u64, u64)> {
    let mut used = std::collections::HashSet::new();
    let mut entries = Vec::new();
    let mut total = 0u64;
    let mut files = 0u64;
    let mut skipped = 0u64;

    for inp in inputs {
        let meta = fs::symlink_metadata(inp).map_err(|e| io_err(e, inp))?;
        if meta.file_type().is_symlink() {
            skipped += 1;
            continue;
        }
        let base = inp
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| WispZipError::Other(format!("invalid input path: {}", inp.display())))?;
        if exclude_hidden && base.starts_with('.') {
            skipped += 1;
            continue;
        }
        let base = dedup(&mut used, base);

        if !meta.is_dir() {
            entries.push(PlanEntry {
                src: inp.clone(),
                name: base,
                size: meta.len(),
                is_dir: false,
                mtime: meta.modified().unwrap_or(UNIX_EPOCH),
                mode: unix_mode(&meta),
            });
            total += meta.len();
            files += 1;
            continue;
        }

        entries.push(PlanEntry {
            src: inp.clone(),
            name: format!("{base}/"),
            size: 0,
            is_dir: true,
            mtime: meta.modified().unwrap_or(UNIX_EPOCH),
            mode: unix_mode(&meta),
        });
        for dent in WalkDir::new(inp).sort_by_file_name() {
            let Ok(de) = dent else {
                continue;
            };
            // depth 0 is the root we already added above
            if de.depth() == 0 {
                continue;
            }
            let m = fs::symlink_metadata(de.path()).map_err(|e| io_err(e, de.path()))?;
            if m.file_type().is_symlink() {
                skipped += 1;
                continue;
            }
            if exclude_hidden
                && de
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with('.'))
            {
                skipped += 1;
                if de.file_type().is_dir() {
                    // don't even descend into hidden directories
                    // (walkdir has no runtime pruning here; skipping entries
                    // is enough because hidden children carry the dot too)
                }
                continue;
            }
            let rel = de.path().strip_prefix(inp).unwrap_or(de.path());
            let rel = rel.to_string_lossy().replace('\\', "/");
            let name = if rel.is_empty() {
                base.clone()
            } else {
                format!("{base}/{rel}")
            };
            if de.file_type().is_dir() {
                entries.push(PlanEntry {
                    src: de.path().to_path_buf(),
                    name: format!("{name}/"),
                    size: 0,
                    is_dir: true,
                    mtime: m.modified().unwrap_or(UNIX_EPOCH),
                    mode: unix_mode(&m),
                });
            } else {
                entries.push(PlanEntry {
                    src: de.path().to_path_buf(),
                    name,
                    size: m.len(),
                    is_dir: false,
                    mtime: m.modified().unwrap_or(UNIX_EPOCH),
                    mode: unix_mode(&m),
                });
                total += m.len();
                files += 1;
            }
        }
    }
    Ok((entries, total, files, skipped))
}

fn dedup(used: &mut std::collections::HashSet<String>, name: String) -> String {
    let mut candidate = name.clone();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), Some(name[i + 1..].to_string())),
        _ => (name.clone(), None),
    };
    let mut i = 1u32;
    while used.contains(&candidate) {
        candidate = match &ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        i += 1;
    }
    used.insert(candidate.clone());
    candidate
}

#[cfg(unix)]
fn unix_mode(m: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(m.mode() & 0o7777)
}

#[cfg(not(unix))]
fn unix_mode(_m: &fs::Metadata) -> Option<u32> {
    None
}
