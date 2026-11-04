use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::error::{io_err, Result, WispZipError};

/// Supported archive / stream formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Gz,
    Zst,
    Xz,
    Bz2,
    SevenZ,
    Rar,
}

impl Format {
    pub const ALL: &'static [Format] = &[
        Format::Zip,
        Format::Tar,
        Format::TarGz,
        Format::TarBz2,
        Format::TarXz,
        Format::TarZst,
        Format::Gz,
        Format::Zst,
        Format::Xz,
        Format::Bz2,
        Format::SevenZ,
        Format::Rar,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Format::Zip => "ZIP",
            Format::Tar => "TAR",
            Format::TarGz => "TAR.GZ",
            Format::TarBz2 => "TAR.BZ2",
            Format::TarXz => "TAR.XZ",
            Format::TarZst => "TAR.ZST",
            Format::Gz => "GZIP",
            Format::Zst => "ZSTD",
            Format::Xz => "XZ",
            Format::Bz2 => "BZIP2",
            Format::SevenZ => "7Z",
            Format::Rar => "RAR",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
            Format::TarBz2 => "tar.bz2",
            Format::TarXz => "tar.xz",
            Format::TarZst => "tar.zst",
            Format::Gz => "gz",
            Format::Zst => "zst",
            Format::Xz => "xz",
            Format::Bz2 => "bz2",
            Format::SevenZ => "7z",
            Format::Rar => "rar",
        }
    }

    /// Whether this format wraps a tar container (as opposed to being a bare
    /// stream or a self-describing archive).
    pub fn is_tar_based(self) -> bool {
        matches!(
            self,
            Format::Tar | Format::TarGz | Format::TarBz2 | Format::TarXz | Format::TarZst
        )
    }

    pub fn supports_password(self) -> bool {
        matches!(self, Format::Zip | Format::SevenZ | Format::Rar)
    }

    pub fn level_range(self) -> (u8, u8) {
        match self {
            Format::Zst => (1, 22),
            Format::TarBz2 | Format::Bz2 => (1, 9),
            _ => (0, 9),
        }
    }

    fn classification(self) -> Classification {
        match self {
            Format::Gz | Format::Zst | Format::Xz | Format::Bz2 => Classification::SingleStream,
            _ => Classification::Archive,
        }
    }

    /// Infer the format from an archive file name. Longest suffixes first so
    /// that `a.tar.gz` resolves to TarGz, not Gz.
    pub fn from_archive_name(name: &str) -> Option<Format> {
        let lower = name.to_ascii_lowercase();
        const SUFFIXES: &[(&str, Format)] = &[
            (".tar.gz", Format::TarGz),
            (".tar.bz2", Format::TarBz2),
            (".tar.xz", Format::TarXz),
            (".tar.zst", Format::TarZst),
            (".tgz", Format::TarGz),
            (".tbz2", Format::TarBz2),
            (".tbz", Format::TarBz2),
            (".txz", Format::TarXz),
            (".tzst", Format::TarZst),
            (".zip", Format::Zip),
            (".7z", Format::SevenZ),
            (".rar", Format::Rar),
            (".tar", Format::Tar),
            (".gz", Format::Gz),
            (".zstd", Format::Zst),
            (".zst", Format::Zst),
            (".xz", Format::Xz),
            (".bz2", Format::Bz2),
        ];
        SUFFIXES
            .iter()
            .find(|(suffix, _)| lower.ends_with(suffix))
            .map(|(_, f)| *f)
    }
}

/// Archives carry a directory of entries; single streams wrap exactly one
/// file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Archive,
    SingleStream,
}

/// Detect the format of `path` by extension first, falling back to magic
/// bytes. Bare streams (.gz/.zst/.xz/.bz2 without a tar sibling) are
/// reported as single streams rather than guessed as archives.
pub fn detect(path: &Path) -> Result<(Format, Classification)> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(f) = Format::from_archive_name(&name) {
        return Ok((f, f.classification()));
    }
    sniff(path)
}

/// Infer the output format from the desired output file name.
pub fn infer_output_format(out: &Path) -> Result<Format> {
    let name = out
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Format::from_archive_name(&name).ok_or_else(|| {
        WispZipError::UnsupportedFormat(format!(
            "cannot infer archive format from {:?}; use an extension like .zip / .tar.gz / .7z",
            name
        ))
    })
}

fn sniff(path: &Path) -> Result<(Format, Classification)> {
    let mut buf = [0u8; 262];
    let mut f = std::fs::File::open(path).map_err(|e| io_err(e, path))?;
    let n = read_up_to(&mut f, &mut buf).map_err(|e| io_err(e, path))?;
    let b = &buf[..n];

    let starts = |magic: &[u8]| b.starts_with(magic);
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") || starts(b"PK\x07\x08") {
        return Ok((Format::Zip, Classification::Archive));
    }
    if starts(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Ok((Format::SevenZ, Classification::Archive));
    }
    // RAR4: "Rar!\x1a\x07\x00", RAR5: "Rar!\x1a\x07\x01\x00"
    if starts(b"Rar!\x1a\x07") {
        return Ok((Format::Rar, Classification::Archive));
    }
    if starts(&[0x1F, 0x8B]) {
        return Ok((Format::Gz, Classification::SingleStream));
    }
    // xz: FD 37 7A 58 5A 00 ("7zXZ" — note the lowercase z)
    if starts(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        return Ok((Format::Xz, Classification::SingleStream));
    }
    // bzip2: "BZh" followed by a block-size digit 1-9
    if b.len() >= 4 && &b[0..3] == b"BZh" && (b'1'..=b'9').contains(&b[3]) {
        return Ok((Format::Bz2, Classification::SingleStream));
    }
    if b.len() >= 262 && &b[257..262] == b"ustar" {
        return Ok((Format::Tar, Classification::Archive));
    }
    Err(WispZipError::UnknownFormat(path.to_path_buf()))
}

fn read_up_to(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::Read;
    let mut filled = 0;
    while filled < buf.len() {
        match f.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_archive_name_matches_longest_suffix() {
        assert_eq!(Format::from_archive_name("a.tar.gz"), Some(Format::TarGz));
        assert_eq!(Format::from_archive_name("A.ZIP"), Some(Format::Zip));
        assert_eq!(Format::from_archive_name("x.tgz"), Some(Format::TarGz));
        assert_eq!(Format::from_archive_name("x.zst"), Some(Format::Zst));
        assert_eq!(Format::from_archive_name("x.xz"), Some(Format::Xz));
        assert_eq!(Format::from_archive_name("x.bz2"), Some(Format::Bz2));
        // tar combos win over bare-stream suffixes
        assert_eq!(Format::from_archive_name("a.tar.xz"), Some(Format::TarXz));
        assert_eq!(Format::from_archive_name("a.tar.bz2"), Some(Format::TarBz2));
        assert_eq!(Format::from_archive_name("x.txt"), None);
    }

    #[test]
    fn infer_output_format_rejects_unknown() {
        assert!(infer_output_format(Path::new("/tmp/out.zip")).is_ok());
        assert!(infer_output_format(Path::new("/tmp/out.bin")).is_err());
    }
}

#[cfg(test)]
mod sniff_tests {
    use super::*;

    #[test]
    fn sniff_detects_xz_and_bz2_magic() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let xp = dir.path().join("s");
        std::fs::File::create(&xp)
            .unwrap()
            .write_all(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00, 1, 2, 3])
            .unwrap();
        assert!(matches!(sniff(&xp), Ok((Format::Xz, Classification::SingleStream))));

        let mut bp = dir.path().join("b");
        bp.set_file_name("b2");
        std::fs::File::create(&bp)
            .unwrap()
            .write_all(b"BZh9xxxxxxx")
            .unwrap();
        assert!(matches!(sniff(&bp), Ok((Format::Bz2, Classification::SingleStream))));
    }
}

#[cfg(test)]
mod sniff_tests_more {
    use super::*;

    fn probe(bytes: &[u8]) -> Result<(Format, Classification), WispZipError> {
        let dir = std::env::temp_dir().join(format!("wispzip-sniff-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join(format!("probe-{}", bytes.len()));
        std::fs::write(&p, bytes).unwrap();
        let r = sniff(&p);
        let _ = std::fs::remove_file(&p);
        r
    }

    #[test]
    fn sniff_all_magics() {
        use Classification::*;
        // zip local file header
        assert!(matches!(probe(b"PK\x03\x04xxxx"), Ok((Format::Zip, Archive))));
        // zip empty archive (end-of-central-directory only)
        assert!(matches!(probe(b"PK\x05\x06xxxx"), Ok((Format::Zip, Archive))));
        // 7z
        assert!(matches!(
            probe([0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C].as_slice()),
            Ok((Format::SevenZ, Archive))
        ));
        // gzip
        assert!(matches!(
            probe([0x1F, 0x8B, 0x08, 0x00].as_slice()),
            Ok((Format::Gz, SingleStream))
        ));
        // xz
        assert!(matches!(
            probe([0xFD, b'7', b'z', b'X', b'Z', 0x00, 0x00].as_slice()),
            Ok((Format::Xz, SingleStream))
        ));
        // bzip2 block sizes 1..9 all valid
        for level in b'1'..=b'9' {
            let magic = [b'B', b'Z', b'h', level];
            assert!(
                matches!(probe(magic.as_slice()), Ok((Format::Bz2, SingleStream))),
                "BZh{} should sniff as bz2",
                level as char
            );
        }
        // "BZh0" is invalid
        assert!(probe(b"BZh0xxxx").is_err());
        // RAR4 vs RAR5 share the prefix
        assert!(matches!(probe(b"Rar!\x1a\x07\x00"), Ok((Format::Rar, _))));
        assert!(matches!(probe(b"Rar!\x1a\x07\x01\x00"), Ok((Format::Rar, _))));
    }

    #[test]
    fn sniff_tar_magic_lives_at_offset_257() {
        let mut block = vec![0u8; 300];
        block[257..262].copy_from_slice(b"ustar");
        let dir = std::env::temp_dir().join(format!("wispzip-sniff-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("tarprobe");
        std::fs::write(&p, &block).unwrap();
        let r = sniff(&p).unwrap();
        assert!(matches!(r, (Format::Tar, Classification::Archive)));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn sniff_tiny_files_do_not_panic() {
        assert!(probe(b"").is_err());
        assert!(probe(b"R").is_err());
        assert!(probe(b"PK").is_err());
    }

    #[test]
    fn level_ranges_documented() {
        assert_eq!(Format::Zst.level_range(), (1, 22));
        assert_eq!(Format::Bz2.level_range(), (1, 9));
        assert_eq!(Format::TarBz2.level_range(), (1, 9));
        assert_eq!(Format::Zip.level_range(), (0, 9));
        assert_eq!(Format::Xz.level_range(), (0, 9));
    }

    #[test]
    fn display_names_and_extensions_agree() {
        for f in Format::ALL {
            let ext = f.extension();
            assert_eq!(Format::from_archive_name(&format!("x.{ext}")), Some(*f));
            assert!(!f.display_name().is_empty());
        }
    }
}
