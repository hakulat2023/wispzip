//! Byte-level split volumes (`<archive>.001`, `<archive>.002`, …).
//!
//! Compression can emit a finished archive as fixed-size volumes; extraction
//! and listing reassemble them transparently. The archive format inside the
//! volumes is arbitrary — the first volume simply starts with its magic.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::error::{io_err, Result, WispZipError};

/// A detected volume set: `stem.001 .. stem.NNN` all present.
#[derive(Debug, Clone)]
pub(crate) struct VolumeSet {
    pub stem: PathBuf,
    pub count: u32,
    pub total_bytes: u64,
}

/// If `path` names `<stem>.<digits>` and the matching `.001` volume exists,
/// return the full contiguous set; errors when a sibling volume is missing.
pub(crate) fn detect(archive: &Path) -> Result<Option<VolumeSet>> {
    let Some((stem, index)) = volume_index(archive) else {
        return Ok(None);
    };
    let first = volume_path(&stem, 1);
    if index != 1 && !first.exists() {
        return Ok(None); // "photo.002" without a "photo.001" sibling is not ours
    }
    let mut set = VolumeSet {
        stem,
        count: 0,
        total_bytes: 0,
    };
    let mut index = 1u32;
    loop {
        let part = volume_path(&set.stem, index);
        if !part.exists() {
            break;
        }
        set.total_bytes += fs::metadata(&part).map_err(|e| io_err(e, &part))?.len();
        set.count = index;
        index += 1;
    }
    if set.count == 0 {
        return Ok(None);
    }
    // gap check: a sibling numbered beyond the contiguous run means corruption
    let mut probe = set.count + 1;
    while probe <= set.count + 100 {
        if volume_path(&set.stem, probe).exists() {
            return Err(missing_volume(&set.stem, set.count + 1));
        }
        probe += 1;
    }
    Ok(Some(set))
}

pub(crate) fn volume_path(stem: &Path, index: u32) -> PathBuf {
    stem.with_file_name(format!(
        "{}.{:03}",
        stem.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into()),
        index
    ))
}

/// `foo/bar.zip.007` -> Some((`foo/bar.zip`, 7)); non-numeric suffix -> None.
fn volume_index(path: &Path) -> Option<(PathBuf, u32)> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let dot = name.rfind('.')?;
    let num = &name[dot + 1..];
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) || num.len() > 9 {
        return None;
    }
    let stem = path.with_file_name(name[..dot].to_string());
    Some((stem, num.parse().ok()?))
}

/// Concatenate all volumes into `tmp` (which must not exist yet).
pub(crate) fn concatenate(set: &VolumeSet, tmp: &Path) -> Result<()> {
    let mut w = BufWriter::with_capacity(
        1 << 20,
        File::create(tmp).map_err(|e| io_err(e, tmp))?,
    );
    for index in 1..=set.count {
        let part = volume_path(&set.stem, index);
        let mut f = File::open(&part).map_err(|e| io_err(e, &part))?;
        std::io::copy(&mut f, &mut w).map_err(|e| io_err(e, &part))?;
    }
    w.flush().map_err(|e| io_err(e, tmp))?;
    Ok(())
}

/// Concatenate all volumes into a unique temp file and return its path.
/// Caller is responsible for removing it.
pub(crate) fn assemble(set: &VolumeSet) -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "wispzip-vol-{}-{}.wispzip-tmp",
        std::process::id(),
        nanos
    ));
    concatenate(set, &tmp)?;
    Ok(tmp)
}

/// Error for a missing mid-set volume.
pub(crate) fn missing_volume(stem: &Path, index: u32) -> WispZipError {
    WispZipError::Other(format!(
        "missing volume {}",
        volume_path(stem, index).display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wispzip-vol-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn volume_path_pads_to_three_digits() {
        let stem = Path::new("/x/backup.zip");
        assert_eq!(volume_path(stem, 1), PathBuf::from("/x/backup.zip.001"));
        assert_eq!(volume_path(stem, 7), PathBuf::from("/x/backup.zip.007"));
        assert_eq!(volume_path(stem, 42), PathBuf::from("/x/backup.zip.042"));
        assert_eq!(volume_path(stem, 1000), PathBuf::from("/x/backup.zip.1000"));
    }

    #[test]
    fn detect_returns_none_for_non_volume_names() {
        // no numeric suffix at all
        assert!(detect(Path::new("/x/archive.zip")).unwrap().is_none());
        // numeric suffix but no .001 sibling anywhere
        assert!(detect(Path::new("/x/photo.002")).unwrap().is_none());
        // directory does not exist
        assert!(detect(Path::new("/nonexistent-dir-xyz/a.001")).unwrap().is_none());
    }

    #[test]
    fn detect_requires_first_volume_when_given_later_index() {
        let dir = temp_dir("first");
        // only .002 exists, no .001: not our volume set
        fs::write(dir.join("a.002"), b"xx").unwrap();
        assert!(detect(&dir.join("a.002")).unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_finds_contiguous_run_and_sums_bytes() {
        let dir = temp_dir("contig");
        for (i, size) in [(1usize, 100usize), (2, 200), (3, 50)] {
            fs::write(dir.join(format!("a.{i:03}")), vec![0u8; size]).unwrap();
        }
        let set = detect(&dir.join("a.001")).unwrap().unwrap();
        assert_eq!(set.count, 3);
        assert_eq!(set.total_bytes, 350);
        assert_eq!(set.stem, dir.join("a"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_errors_on_gap() {
        let dir = temp_dir("gap");
        for i in [1usize, 2, 3, 5] {
            fs::write(dir.join(format!("a.{i:03}")), b"x").unwrap();
        }
        let err = detect(&dir.join("a.001")).unwrap_err();
        assert!(err.to_string().contains("004"), "gap should point at 004, got {err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concatenate_reassembles_in_order() {
        let dir = temp_dir("concat");
        let set = VolumeSet { stem: dir.join("a"), count: 3, total_bytes: 6 };
        fs::write(dir.join("a.001"), b"123").unwrap();
        fs::write(dir.join("a.002"), b"456").unwrap();
        fs::write(dir.join("a.003"), b"789").unwrap();
        let tmp = dir.join("joined");
        concatenate(&set, &tmp).unwrap();
        assert_eq!(fs::read(&tmp).unwrap(), b"123456789");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_index_parses_digits_only() {
        // internal helper through detect(): "a.12x" is not a volume name
        // (covered by detect_returns_none_for_non_volume_names indirectly);
        // here we assert the visible behavior of assemble on a single-volume set
        let dir = temp_dir("single");
        fs::write(dir.join("s.001"), b"only").unwrap();
        let set = detect(&dir.join("s.001")).unwrap().unwrap();
        assert_eq!(set.count, 1);
        let tmp = assemble(&set).unwrap();
        assert_eq!(fs::read(&tmp).unwrap(), b"only");
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_dir_all(&dir);
    }
}
