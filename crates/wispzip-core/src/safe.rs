use std::path::{Component, Path, PathBuf};

use crate::error::{Result, WispZipError};

/// Join an archive entry name onto `dest`, rejecting anything that escapes
/// the destination: absolute paths, `..` components, windows drive prefixes.
/// Backslashes are normalized to `/` so archives written on Windows unpack
/// the same way.
pub fn safe_join(dest: &Path, entry: &str) -> Result<PathBuf> {
    if entry.contains('\0') {
        return Err(WispZipError::UnsafeEntry(entry.to_string()));
    }
    let normalized = entry.replace('\\', "/");
    // Windows drive prefixes ("C:/...") and NTFS alternate-data-stream
    // separators both hinge on ':'. A colon anywhere in an entry name cannot
    // be written on Windows, so reject it uniformly on every platform.
    if normalized.contains(':') {
        return Err(WispZipError::UnsafeEntry(entry.to_string()));
    }
    let mut out = dest.to_path_buf();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return Err(WispZipError::UnsafeEntry(entry.to_string())),
        }
    }
    Ok(out)
}

/// `name.ext` -> `name (1).ext` -> `name (2).ext`, skipping existing paths.
pub fn unique_path(p: &Path) -> PathBuf {
    if !p.symlink_metadata().is_ok() {
        return p.to_path_buf();
    }
    let dir = p.parent().unwrap_or(Path::new("."));
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], Some(&name[i + 1..])),
        _ => (name.as_str(), None),
    };
    for i in 1u32.. {
        let candidate = match ext {
            Some(e) => dir.join(format!("{stem} ({i}).{e}")),
            None => dir.join(format!("{stem} ({i})")),
        };
        if !candidate.symlink_metadata().is_ok() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_traversal() {
        let dest = Path::new("/tmp/wispzip-dest");
        assert!(safe_join(dest, "../evil.txt").is_err());
        assert!(safe_join(dest, "/etc/passwd").is_err());
        assert!(safe_join(dest, "a/../../evil").is_err());
        assert!(safe_join(dest, "C:\\windows\\evil").is_err());
        assert_eq!(
            safe_join(dest, "a/b/c.txt").unwrap(),
            dest.join("a/b/c.txt")
        );
        assert_eq!(safe_join(dest, "./a//b").unwrap(), dest.join("a/b"));
        // windows-style separators normalize
        assert_eq!(safe_join(dest, "a\\b.txt").unwrap(), dest.join("a/b.txt"));
    }

    #[test]
    fn safe_join_edge_cases() {
        let dest = Path::new("/tmp/wispzip-dest");
        // NUL byte
        assert!(safe_join(dest, "a\0b").is_err());
        // parent at the end
        assert!(safe_join(dest, "a/b/..").is_err());
        // bare ".." and "."
        assert!(safe_join(dest, "..").is_err());
        assert_eq!(safe_join(dest, ".").unwrap(), dest);
        // empty entry resolves to dest itself
        assert_eq!(safe_join(dest, "").unwrap(), dest);
        // windows drive with forward slashes
        assert!(safe_join(dest, "C:/evil").is_err());
        // ':' anywhere is rejected (NTFS ADS separator; unwritable on Windows)
        assert!(safe_join(dest, "dir/x:y.txt").is_err());
        assert!(safe_join(dest, "stream:ads").is_err());
        // deep normal path with unicode
        assert_eq!(
            safe_join(dest, "目录/中文.txt").unwrap(),
            dest.join("目录/中文.txt")
        );
    }

    #[test]
    fn unique_path_no_extension_and_hidden_file() {
        let dir = std::env::temp_dir().join(format!("wispzip-uniq2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // no extension
        let p = dir.join("Makefile");
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(unique_path(&p), dir.join("Makefile (1)"));
        // hidden file: the leading dot is not an extension separator, so the
        // whole name is the stem -> ".gitignore (1)"
        let h = dir.join(".gitignore");
        std::fs::write(&h, b"x").unwrap();
        assert_eq!(unique_path(&h), dir.join(".gitignore (1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unique_path_returns_original_when_free() {
        let dir = std::env::temp_dir().join(format!("wispzip-uniq3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("free.txt");
        assert_eq!(unique_path(&p), p);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unique_path_increments() {
        let dir = std::env::temp_dir().join(format!("wispzip-uniq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("name.txt");
        std::fs::write(&p, b"x").unwrap();
        let u1 = unique_path(&p);
        assert_eq!(u1, dir.join("name (1).txt"));
        std::fs::write(&u1, b"x").unwrap();
        assert_eq!(unique_path(&p), dir.join("name (2).txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
