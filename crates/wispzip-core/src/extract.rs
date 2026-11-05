//! Extraction orchestration: archive detection, per-format dispatch, and the
//! shared on-disk machinery (conflict policy, zip-slip-safe targets, metadata
//! restore) that every format module builds on.

use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{io_err, Result, WispZipError};
use crate::format::{Classification, Format};
use crate::progress::{new_cancel_flag, Phase, Progress, Reporter};
use crate::safe::{safe_join, unique_path};

/// What to do when an entry already exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Conflict {
    Skip,
    Overwrite,
    #[default]
    Rename,
}

/// How to decode non-UTF-8 entry names (classic zip stores raw bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NameEncoding {
    #[default]
    Auto,
    Utf8,
    Gbk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractOptions {
    /// Destination directory; created if missing.
    pub dest: PathBuf,
    /// Entry names to unpack; `None` unpacks everything.
    #[serde(default)]
    pub entries: Option<Vec<String>>,
    #[serde(default)]
    pub conflict: Conflict,
    #[serde(default)]
    pub encoding: NameEncoding,
    /// Password for encrypted zip / 7z / rar archives.
    #[serde(default)]
    pub password: Option<String>,
    /// Path prefix to strip from entry names before writing to disk.
    /// `Some("Mango")` turns archive entry `Mango/a.docx` into `a.docx`.
    /// Used for partial extraction so files land directly in `dest` without
    /// recreating the archive's internal directory hierarchy.
    #[serde(default, rename = "stripPrefix")]
    pub strip_prefix: Option<String>,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            dest: PathBuf::new(),
            entries: None,
            conflict: Conflict::Rename,
            encoding: NameEncoding::Auto,
            password: None,
            strip_prefix: None,
        }
    }
}

impl ExtractOptions {
    /// Strip `strip_prefix` from an entry name. `"Mango/deep/a.txt"` with
    /// prefix `"Mango"` becomes `"deep/a.txt"`. Only matches on path-component
    /// boundaries: prefix `"Mango"` does NOT strip from `"Mangoes/a.txt"`.
    pub(crate) fn strip_name<'a>(&self, name: &'a str) -> &'a str {
        match &self.strip_prefix {
            Some(prefix) if !prefix.is_empty() => {
                let trimmed = name.trim_start_matches('/');
                if trimmed == prefix {
                    ""
                } else if let Some(rest) = trimmed.strip_prefix(prefix) {
                    // Only strip when followed by '/' (component boundary),
                    // so prefix "Mango" does not match "Mangoes/...".
                    rest.strip_prefix('/').unwrap_or(name)
                } else {
                    name
                }
            }
            _ => name,
        }
    }

    /// `safe_join(dest, strip_name(name))` — convenience for format modules.
    pub(crate) fn join_name(&self, name: &str) -> Result<PathBuf> {
        safe_join(&self.dest, self.strip_name(name))
    }

    /// `resolve_target` that accounts for `strip_prefix`.
    pub(crate) fn resolve(&self, name: &str) -> Result<TargetAction> {
        resolve_target(&self.dest, self.strip_name(name), self.conflict)
    }
}

#[derive(Debug, Clone)]
pub struct ExtractStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub skipped: u64,
    pub dest: PathBuf,
}

/// Extract `archive` into `opt.dest`.
///
/// Split volumes (`<archive>.001`, …) are read as one contiguous stream, so no
/// full-size temporary copy is made.
pub fn extract<F>(archive: &Path, opt: &ExtractOptions, cb: F) -> Result<ExtractStats>
where
    F: FnMut(&Progress) -> bool,
{
    let source = match crate::volumes::detect(archive)? {
        Some(set) => crate::source::ArchiveSource::from_volumes(set),
        None => crate::source::ArchiveSource::from_path(archive)?,
    };
    let (format, class) = crate::format::detect_source(&source)?;
    fs::create_dir_all(&opt.dest).map_err(|e| io_err(e, &opt.dest))?;

    let shared = Reporter::new(new_cancel_flag(), cb);
    {
        let mut r = shared.borrow_mut();
        r.phase = Phase::Extract;
        // Known totals: tar pipelines count compressed stream position; zip
        // totals are summed from the central directory; 7z is indeterminate.
        r.total = match class {
            Classification::SingleStream => Some(source.len()),
            _ if format.is_tar_based() => Some(source.len()),
            _ => None,
        };
        r.report(true);
    }

    let mut stats = ExtractStats {
        files: 0,
        dirs: 0,
        bytes: 0,
        skipped: 0,
        dest: opt.dest.clone(),
    };

    match format {
        Format::Zip => crate::formats::zip::extract(&source, opt, &shared, &mut stats)?,
        Format::SevenZ => crate::formats::sevenz::extract(&source, opt, &shared, &mut stats)?,
        // UnRAR only accepts a real file path, so a split rar set still has to
        // be concatenated into a temporary archive first.
        Format::Rar => match &source {
            crate::source::ArchiveSource::File { path, .. } => {
                crate::formats::rar::extract(path, opt, &shared, &mut stats)?
            }
            crate::source::ArchiveSource::Volumes(set) => {
                let tmp = crate::volumes::assemble(set)?;
                let result = crate::formats::rar::extract(&tmp, opt, &shared, &mut stats);
                let _ = fs::remove_file(&tmp);
                result?
            }
        },
        Format::Gz | Format::Zst | Format::Xz | Format::Bz2 => {
            crate::formats::streams::extract(&source, opt, format, &shared, &mut stats)?
        }
        _ => crate::formats::tar::extract(&source, opt, format, &shared, &mut stats)?,
    }

    {
        let mut r = shared.borrow_mut();
        r.phase = Phase::Finish;
        r.report(true);
    }
    Ok(stats)
}

// ---- shared helpers (used by the formats/ modules) --------------------------

pub(crate) enum TargetAction {
    Write(PathBuf),
    Skip,
}

pub(crate) fn resolve_target(dest: &Path, name: &str, conflict: Conflict) -> Result<TargetAction> {
    let mut target = safe_join(dest, name)?;
    if target.symlink_metadata().is_ok() {
        match conflict {
            Conflict::Skip => return Ok(TargetAction::Skip),
            Conflict::Overwrite => {
                if target.is_dir() {
                    return Err(WispZipError::Other(format!(
                        "cannot overwrite directory with a file: {}",
                        target.display()
                    )));
                }
                fs::remove_file(&target).map_err(|e| io_err(e, &target))?;
            }
            Conflict::Rename => target = unique_path(&target),
        }
    }
    Ok(TargetAction::Write(target))
}

pub(crate) fn ensure_parent(target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| io_err(e, parent))?;
        }
    }
    Ok(())
}

/// Per-extraction cache of directories that have already been created.
///
/// `create_dir_all` stats (and on some platforms tries to `mkdir`) every path
/// component. A source tarball with thousands of files nested 6-9 levels deep
/// otherwise repeats that whole walk once per file, which is pure overhead.
/// The cache lives for a single extraction, so a later job never sees stale
/// entries.
pub(crate) struct ParentDirs(HashSet<PathBuf>);

impl ParentDirs {
    pub(crate) fn new() -> Self {
        Self(HashSet::new())
    }

    /// Create `dir` (including parents) only the first time it is requested.
    pub(crate) fn create(&mut self, dir: &Path) -> Result<()> {
        if !dir.as_os_str().is_empty() && self.0.insert(dir.to_path_buf()) {
            fs::create_dir_all(dir).map_err(|e| io_err(e, dir))?;
        }
        Ok(())
    }

    /// Ensure the parent directory of `target` exists.
    pub(crate) fn ensure(&mut self, target: &Path) -> Result<()> {
        match target.parent() {
            Some(p) => self.create(p),
            None => Ok(()),
        }
    }
}

/// Tracks the selected entries during extraction so iteration can stop as
/// soon as every selection has been handled, instead of scanning the rest of
/// the archive. Selections are exact file names (folders are expanded by the
/// caller); a `None` selection extracts everything.
pub(crate) struct EntrySelection {
    wanted: Vec<String>,
    done: Vec<bool>,
    all: bool,
}

impl EntrySelection {
    pub fn new(entries: &Option<Vec<String>>) -> Self {
        match entries {
            Some(v) => Self {
                // Normalize filter names so they compare against the traversal entry names (post-normalize)
                wanted: v.iter().map(|w| normalize_entry_name(w)).collect(),
                done: vec![false; v.len()],
                all: false,
            },
            None => Self {
                wanted: Vec::new(),
                done: Vec::new(),
                all: true,
            },
        }
    }

    /// Is this entry part of the selection? `None` selection selects all.
    /// Normalize an entry name before comparison (some archives use '\' as the separator).
    pub fn selects(&self, name: &str) -> bool {
        self.all || self.wanted.iter().any(|w| *w == normalize_entry_name(name))
    }

    /// Mark an entry handled. Returns true once every selected entry has been
    /// seen and the iteration can stop early.
    pub fn settled(&mut self, name: &str) -> bool {
        if self.all {
            return false;
        }
        let name = normalize_entry_name(name);
        for (i, w) in self.wanted.iter().enumerate() {
            if !self.done[i] && *w == name {
                self.done[i] = true;
            }
        }
        self.done.iter().all(|d| *d)
    }
}

pub(crate) fn set_file_metadata(f: &File, mtime: Option<SystemTime>, mode: Option<u32>) {
    if let Some(t) = mtime {
        let _ = f.set_modified(t);
    }
    #[cfg(unix)]
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(fs::Permissions::from_mode(m & 0o7777));
    }
    #[cfg(not(unix))]
    let _ = mode;
}

pub(crate) fn normalize_entry_name(name: &str) -> String {
    let name = name.trim_start_matches("./");
    // Some Windows packers use '\' as the entry separator (the ZIP spec requires '/').
    // Unify to '/' so name filtering, display and on-disk paths (safe_join) all agree.
    let name = name.replace('\\', "/");
    name.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_name_no_prefix_returns_original() {
        let opt = ExtractOptions::default();
        assert_eq!(opt.strip_name("Mango/a.txt"), "Mango/a.txt");
    }

    #[test]
    fn strip_name_exact_match_returns_empty() {
        let opt = ExtractOptions {
            strip_prefix: Some("Mango".into()),
            ..Default::default()
        };
        assert_eq!(opt.strip_name("Mango"), "");
    }

    #[test]
    fn strip_name_under_prefix_strips_it() {
        let opt = ExtractOptions {
            strip_prefix: Some("Mango".into()),
            ..Default::default()
        };
        assert_eq!(opt.strip_name("Mango/a.txt"), "a.txt");
        assert_eq!(opt.strip_name("Mango/deep/b.txt"), "deep/b.txt");
    }

    #[test]
    fn strip_name_unrelated_prefix_unchanged() {
        let opt = ExtractOptions {
            strip_prefix: Some("Mango".into()),
            ..Default::default()
        };
        assert_eq!(opt.strip_name("Banana/a.txt"), "Banana/a.txt");
    }

    #[test]
    fn strip_name_similar_but_not_exact_prefix_unchanged() {
        let opt = ExtractOptions {
            strip_prefix: Some("Mango".into()),
            ..Default::default()
        };
        // "Mangoes" starts with "Mango" but is a different directory
        assert_eq!(opt.strip_name("Mangoes/a.txt"), "Mangoes/a.txt");
    }
}
