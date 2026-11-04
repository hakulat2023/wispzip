//! End-to-end roundtrip tests: build a source tree, compress it into every
//! supported format, extract, and compare byte-for-byte.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use wispzip_core::{compress, extract, list_archive, CompressOptions, ExtractOptions, Format, WispZipError};

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wispzip-test-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Deterministic pseudo-random bytes (no rand dependency).
fn prng(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed;
    (0..len)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as u8
        })
        .collect()
}

/// Minimal CRC32 (IEEE 802.3, as used by zip) for hand-built fixtures.
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn build_tree(root: &Path) -> PathBuf {
    let src = root.join("src-tree");
    fs::create_dir_all(src.join("nested/deep")).unwrap();
    fs::create_dir_all(src.join("nested/empty")).unwrap();
    fs::write(src.join("nested/a.txt"), "hello wispzip\n".repeat(200)).unwrap();
    fs::write(src.join("nested/deep/binary.bin"), prng(42, 128 * 1024)).unwrap();
    fs::write(src.join("中文文件.txt"), "中文内容内容内容\n".repeat(50)).unwrap();
    fs::write(src.join("top.md"), "# top\n").unwrap();
    src
}

fn walk_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn rec(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                rec(&p, out);
            } else {
                out.push((p.clone(), fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    rec(dir, &mut out);
    out.sort();
    out
}

fn noop_progress(_: &wispzip_core::Progress) -> bool {
    true
}

fn roundtrip(format: Format) {
    let root = temp_root(&format.extension().replace('.', "-"));
    let src = build_tree(&root);
    let archive = root.join(format!("out.{}", format.extension()));
    let expected = walk_files(&src);

    let stats = compress(
        &[src.clone()],
        &archive,
        &CompressOptions {
            format,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_or_else(|e| panic!("{format:?} compress failed: {e}"));
    assert_eq!(stats.input_files, 4, "{format:?}");
    assert!(archive.is_file(), "{format:?}");

    // listing should see the same names
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None)
        .unwrap_or_else(|e| panic!("{format:?} list failed: {e}"));
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"src-tree/nested/a.txt"), "{format:?} {names:?}");
    assert!(names.contains(&"src-tree/中文文件.txt"), "{format:?} {names:?}");

    // extract and compare
    let dest = root.join("out-dir");
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Overwrite,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_or_else(|e| panic!("{format:?} extract failed: {e}"));
    let got = walk_files(&dest);
    assert_eq!(got.len(), expected.len(), "{format:?}");
    for (exp, got) in expected.iter().zip(got.iter()) {
        let rel_exp = exp.0.strip_prefix(&root).unwrap();
        let rel_got = got.0.strip_prefix(&dest).unwrap();
        assert_eq!(rel_exp, rel_got, "{format:?} name mismatch");
        assert_eq!(exp.1, got.1, "{format:?} content mismatch for {rel_exp:?}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn roundtrip_zip() {
    roundtrip(Format::Zip);
}

#[test]
fn roundtrip_tar() {
    roundtrip(Format::Tar);
}

#[test]
fn roundtrip_tar_gz() {
    roundtrip(Format::TarGz);
}

#[test]
fn roundtrip_tar_bz2() {
    roundtrip(Format::TarBz2);
}

#[test]
fn roundtrip_tar_xz() {
    roundtrip(Format::TarXz);
}

#[test]
fn roundtrip_tar_zst() {
    roundtrip(Format::TarZst);
}

#[test]
fn roundtrip_7z() {
    roundtrip(Format::SevenZ);
}

fn single_stream_roundtrip(format: Format) {
    let root = temp_root(&format.extension());
    let src = root.join("payload.bin");
    let data = prng(7, 300 * 1024);
    fs::write(&src, &data).unwrap();

    let archive = root.join(format!("payload.bin.{}", format.extension()));
    compress(
        &[src.clone()],
        &archive,
        &CompressOptions {
            format,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    assert_eq!(entries[0].name, "payload.bin");

    let dest = root.join("out");
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(fs::read(dest.join("payload.bin")).unwrap(), data);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn roundtrip_gz_single() {
    single_stream_roundtrip(Format::Gz);
}

#[test]
fn roundtrip_zst_single() {
    single_stream_roundtrip(Format::Zst);
}

#[test]
fn roundtrip_xz_single() {
    single_stream_roundtrip(Format::Xz);
}

#[test]
fn roundtrip_bz2_single() {
    single_stream_roundtrip(Format::Bz2);
}

#[test]
fn single_stream_magic_sniff() {
    // A bare stream without a known extension is detected by magic bytes and
    // extracted to "<name>.out".
    for (format, data) in [
        (Format::Xz, b"xz sniffed content\n".repeat(64)),
        (Format::Bz2, b"bz2 sniffed content\n".repeat(64)),
    ] {
        let root = temp_root(&format!("sniff-{}", format.extension()));
        let src = root.join("payload.bin");
        fs::write(&src, &data).unwrap();
        let archive = root.join(format!("stream.{}", format.extension()));
        compress(
            &[src],
            &archive,
            &CompressOptions { format, ..Default::default() },
            noop_progress,
        )
        .unwrap();

        // strip the extension: detection must fall back to magic bytes
        let bare = root.join("stream");
        fs::rename(&archive, &bare).unwrap();

        let dest = root.join("out");
        extract(
            &bare,
            &ExtractOptions { dest: dest.clone(), ..Default::default() },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{format:?} sniffed extract failed: {e}"));
        assert_eq!(fs::read(dest.join("stream.out")).unwrap(), data, "{format:?}");
        let _ = fs::remove_dir_all(&root);
    }
}

#[test]
fn zip_password_roundtrip_and_rejection() {
    let root = temp_root("zip-pw");
    let src = root.join("secret.txt");
    fs::write(&src, "classified\n".repeat(100)).unwrap();
    let archive = root.join("secret.zip");

    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::Zip,
            password: Some("p@ss中文".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    // wrong password -> error mentioning password
    let wrong = extract(
        &archive,
        &ExtractOptions {
            dest: root.join("out-wrong"),
            password: Some("nope".into()),
            ..Default::default()
        },
        noop_progress,
    );
    assert!(wrong.is_err(), "wrong password must fail");

    // correct password -> roundtrip
    extract(
        &archive,
        &ExtractOptions {
            dest: root.join("out-ok"),
            password: Some("p@ss中文".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(
        fs::read(root.join("out-ok/secret.txt")).unwrap(),
        b"classified\n".repeat(100)
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn tar_zip_slip_is_rejected() {
    let root = temp_root("slip");
    let archive = root.join("evil.tar");
    // Hand-craft a ustar entry named "../evil.txt" (tar::Builder refuses to).
    {
        use std::io::Write as _;
        let mut header = [0u8; 512];
        let name = b"../evil.txt";
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(b"0000644\0"); // mode
        header[108..116].copy_from_slice(b"0000000\0"); // uid
        header[116..124].copy_from_slice(b"0000000\0"); // gid
        header[124..136].copy_from_slice(b"00000000005\0"); // size = 5
        header[136..148].copy_from_slice(b"00000000000\0"); // mtime
        header[156] = b'0'; // regular file
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].copy_from_slice(b"        "); // chksum field = spaces
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut f = fs::File::create(&archive).unwrap();
        f.write_all(&header).unwrap();
        let mut data = [0u8; 512];
        data[..5].copy_from_slice(b"PWNED");
        f.write_all(&data).unwrap();
        f.write_all(&[0u8; 1024]).unwrap(); // end-of-archive blocks
    }
    let dest = root.join("dest");
    let err = extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_err();
    assert!(err.is_unsafe_entry(), "expected unsafe entry, got {err}");
    assert!(!root.join("evil.txt").exists(), "must not escape dest");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn conflict_policies() {
    let root = temp_root("conflict");
    let src = root.join("f.txt");
    fs::write(&src, "v1").unwrap();
    let archive = root.join("f.zip");
    compress(&[src], &archive, &CompressOptions::default(), noop_progress).unwrap();

    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    // rename policy: second extract must not overwrite
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    assert!(dest.join("f (1).txt").exists());

    // skip policy counts skipped
    let st = extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Skip,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(st.skipped, 1);
    assert_eq!(fs::read(dest.join("f.txt")).unwrap(), b"v1");

    // overwrite policy replaces
    let st = extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Overwrite,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(st.skipped, 0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cancel_midway_leaves_no_output() {
    let root = temp_root("cancel");
    let src = build_tree(&root);
    let archive = root.join("out.zip");
    let err = compress(
        &[src],
        &archive,
        &CompressOptions::default(),
        |_| false, // cancel on first progress report
    )
    .unwrap_err();
    assert!(err.is_cancelled(), "expected cancelled, got {err}");
    assert!(!archive.exists(), "no final output after cancel");
    let leftovers: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".wispzip-part"))
        .collect();
    assert!(leftovers.is_empty(), "partial file must be cleaned up");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn sevenz_extraction_roundtrip() {
    // Build a 7z archive with the same engine family (sevenz-rust writer)
    // so we can verify extraction independently of external tools.
    let root = temp_root("sevenz");
    let src = root.join("s7");
    fs::create_dir_all(src.join("d")).unwrap();
    fs::write(src.join("d/data.bin"), prng(99, 70 * 1024)).unwrap();
    fs::write(src.join("top.txt"), "sevenz top\n".repeat(10)).unwrap();

    let archive = root.join("s7.7z");
    let mut w = sevenz_rust::SevenZWriter::create(&archive).unwrap();
    w.push_source_path(&src, |_| true).unwrap();
    w.finish().unwrap();

    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"top.txt"), "{names:?}");

    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    assert_eq!(
        fs::read(dest.join("top.txt")).unwrap(),
        b"sevenz top\n".repeat(10)
    );
    assert_eq!(
        fs::read(dest.join("d/data.bin")).unwrap(),
        prng(99, 70 * 1024)
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn sevenz_password_roundtrip_and_rejection() {
    let root = temp_root("7z-pw");
    let src = build_tree(&root);
    let archive = root.join("secret.7z");

    compress(
        &[src.clone()],
        &archive,
        &CompressOptions {
            format: Format::SevenZ,
            password: Some("p@ss中文".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    // Plain headers: listing works without the password.
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    assert!(!entries.is_empty());

    // No password -> flagged as missing password, not a generic failure.
    let nopw = extract(
        &archive,
        &ExtractOptions {
            dest: root.join("out-nopw"),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_err();
    assert!(nopw.is_password(), "expected password error, got {nopw}");

    // Wrong password -> rejected.
    let wrong = extract(
        &archive,
        &ExtractOptions {
            dest: root.join("out-wrong"),
            password: Some("nope".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_err();
    assert!(wrong.is_password(), "expected password error, got {wrong}");

    // Correct password -> full roundtrip of the tree.
    let dest = root.join("out-ok");
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            password: Some("p@ss中文".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    let expected = walk_files(&src);
    let got = walk_files(&dest);
    assert_eq!(got.len(), expected.len());
    for (exp, got) in expected.iter().zip(got.iter()) {
        assert_eq!(
            fs::read(&got.0).unwrap(),
            fs::read(&exp.0).unwrap(),
            "content mismatch for {:?}",
            exp.0
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn sevenz_encrypted_is_readable_by_sevenz_reader() {
    // Cross-check our AES-256 output through sevenz-rust's own reader API
    // (independent of wispzip's extract path). `r: &mut dyn Read`, so trait
    // methods resolve without an import.
    let root = temp_root("7z-x");
    let src = root.join("note.txt");
    fs::write(&src, "aes roundtrip\n".repeat(64)).unwrap();
    let archive = root.join("note.7z");
    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::SevenZ,
            password: Some("k3y".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    let mut sz = sevenz_rust::SevenZReader::open(&archive, "k3y".into()).unwrap();
    let mut seen = Vec::new();
    sz.for_each_entries(|e, r| {
        if !e.is_directory {
            let mut buf = Vec::new();
            r.read_to_end(&mut buf).unwrap();
            seen.push((e.name.clone(), buf));
        }
        Ok(true)
    })
    .unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "note.txt");
    assert_eq!(seen[0].1, b"aes roundtrip\n".repeat(64));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn sevenz_encrypted_header_listing_needs_password() {
    // -mhe=on style archive: sevenz-rust's blessed helper encrypts the header
    // by default, so even entry names stay hidden without the password.
    let root = temp_root("7z-mhe");
    let src = root.join("mhe");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("hidden.txt"), "header-encrypted\n".repeat(32)).unwrap();
    let archive = root.join("mhe.7z");
    sevenz_rust::compress_to_path_encrypted(&src, &archive, "mhe-pw".into()).unwrap();

    // no password -> password error, not a generic failure
    let nopw = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap_err();
    assert!(nopw.is_password(), "expected password error, got {nopw}");

    // wrong password -> password error too
    let wrong = list_archive(&archive, wispzip_core::NameEncoding::Auto, Some("nope")).unwrap_err();
    assert!(wrong.is_password(), "expected password error, got {wrong}");

    // with the password -> names are visible
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, Some("mhe-pw")).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.iter().any(|n| n.contains("hidden.txt")), "{names:?}");

    // and extraction still works with the password
    let dest = root.join("out");
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            password: Some("mhe-pw".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(
        fs::read(dest.join("hidden.txt")).unwrap(),
        b"header-encrypted\n".repeat(32)
    );
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn sevenz_preserves_unix_modes() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_root("7z-mode");
    let src = root.join("modes");
    fs::create_dir_all(src.join("d")).unwrap();
    fs::write(src.join("exec.sh"), "#!/bin/sh\necho hi\n").unwrap();
    fs::write(src.join("d/plain.txt"), "plain\n").unwrap();
    fs::set_permissions(src.join("exec.sh"), fs::Permissions::from_mode(0o754)).unwrap();
    fs::set_permissions(src.join("d/plain.txt"), fs::Permissions::from_mode(0o604)).unwrap();

    // our own compress -> extract roundtrip keeps the modes
    let archive = root.join("modes.7z");
    compress(&[src.clone()], &archive, &CompressOptions { format: Format::SevenZ, ..Default::default() }, noop_progress).unwrap();

    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    let mode_of = |needle: &str| {
        entries
            .iter()
            .find(|e| e.name.ends_with(needle))
            .and_then(|e| e.mode)
            .unwrap_or_else(|| panic!("no mode for {needle}: {entries:?}"))
    };
    assert_eq!(mode_of("exec.sh") & 0o7777, 0o754);
    assert_eq!(mode_of("plain.txt") & 0o7777, 0o604);

    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    assert_eq!(
        dest.join("modes/exec.sh").metadata().unwrap().permissions().mode() & 0o7777,
        0o754
    );
    assert_eq!(
        dest.join("modes/d/plain.txt").metadata().unwrap().permissions().mode() & 0o7777,
        0o604
    );

    // real 7-Zip reads our attributes: it must restore the exec bit too
    let via7z = root.join("via7z");
    let ok = std::process::Command::new("7z")
        .args(["x", "-y", "-o"])
        .arg(&via7z)
        .arg(&archive)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        assert_eq!(
            via7z.join("modes/exec.sh").metadata().unwrap().permissions().mode() & 0o777,
            0o754 & 0o777
        );
    }

    // and wispzip extracts real-7z archives keeping their modes
    let from7z = root.join("from7z.7z");
    let ok = std::process::Command::new("7z")
        .args(["a", "-y"])
        .arg(&from7z)
        .arg(&src)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        let dest2 = root.join("out2");
        extract(&from7z, &ExtractOptions { dest: dest2.clone(), ..Default::default() }, noop_progress).unwrap();
        assert_eq!(
            dest2.join("modes/exec.sh").metadata().unwrap().permissions().mode() & 0o7777,
            0o754
        );
    }
    let _ = fs::remove_dir_all(&root);
}

// Fixtures in tests/data/ come from the unrar crate (see tests/data/README.md).

/// Multi-threaded extraction (large zip archives automatically use the worker pool): content must match.
#[test]
fn zip_parallel_extraction_matches_source() {
    let root = temp_root("zip-mt");
    let src = root.join("many");
    fs::create_dir_all(&src).unwrap();
    let mut expected: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..200 {
        let name = format!("f{i:03}.bin");
        let data = prng(1000 + i as u64, 8 * 1024);
        fs::write(src.join(&name), &data).unwrap();
        expected.push((name, data));
    }

    let archive = root.join("many.zip");
    compress(&[src.clone()], &archive, &CompressOptions::default(), noop_progress).unwrap();
    // 200 entries / 1.6MB: above the auto_workers threshold, so the multi-threaded path is always taken
    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();

    for (name, data) in expected.iter() {
        assert_eq!(fs::read(dest.join("many").join(name)).unwrap(), *data, "{name}");
    }
    let _ = fs::remove_dir_all(&root);
}

/// Early termination: corrupt the second half of the archive, then extracting only the leading small
/// entries must still succeed -- proving per-entry extraction never reads/validates the whole archive.
#[test]
fn early_exit_extracts_target_without_reading_rest() {
    let root = temp_root("early-exit");
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("early.txt"), "early payload").unwrap();
    fs::write(src.join("late.bin"), prng(77, 4 * 1024 * 1024)).unwrap();

    fn corrupt_at(archive: &Path, pos: u64) {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = fs::OpenOptions::new().write(true).open(archive).unwrap();
        f.seek(SeekFrom::Start(pos)).unwrap();
        f.write_all(&[0xFF, 0x00, 0xFF, 0x00]).unwrap();
    }

    for format in [Format::Zip, Format::SevenZ, Format::Tar] {
        let archive = root.join(format!("ee.{}", format.extension()));
        compress(&[src.clone()], &archive, &CompressOptions { format, ..Default::default() }, noop_progress).unwrap();

        // Corrupt the trailing region: for zip/7z hit the middle of the late entries' data (len/2);
        // tar is byte-transparent, so hit the tar header of late.bin (offset 512*3, deterministic)
        let len = fs::metadata(&archive).unwrap().len();
        let pos = if format == Format::Tar { 1536 } else { len / 2 };
        corrupt_at(&archive, pos);

        // A full extraction must fail (proving the corrupted region really is read by the normal path)
        let full = extract(
            &archive,
            &ExtractOptions { dest: root.join("full"), ..Default::default() },
            noop_progress,
        );
        assert!(full.is_err(), "{format:?}: corrupted archive must fail full extraction");

        // Extract only the leading small entries: early termination -> the corrupted region is never touched
        let dest = root.join(format!("part-{}", format.extension()));
        extract(&archive, &ExtractOptions {
            dest: dest.clone(),
            entries: Some(vec!["src/early.txt".into()]),
            ..Default::default()
        }, noop_progress)
        .unwrap_or_else(|e| panic!("{format:?}: early-entry extraction must succeed: {e}"));
        assert_eq!(
            fs::read(dest.join("src/early.txt")).unwrap(),
            b"early payload",
            "{format:?}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn extract_conflict_matrix_per_policy() {
    // Full matrix: three policies x (file exists / dir-vs-file collision)
    let root = temp_root("conflict-matrix");
    // a single file input -> the archive contains just "f.txt" (no parent dir)
    let src = root.join("f.txt");
    fs::write(&src, "content-v2").unwrap();
    let archive = root.join("a.zip");
    compress(&[src], &archive, &CompressOptions::default(), noop_progress).unwrap();

    // -- skip: existing content untouched, second file counted as skipped --
    let dest = root.join("skip");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("f.txt"), "existing").unwrap();
    let st = extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Skip,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(st.skipped, 1, "skip policy must count the skipped file");
    assert_eq!(fs::read(dest.join("f.txt")).unwrap(), b"existing");

    // -- overwrite: existing content replaced --
    let dest = root.join("overwrite");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("f.txt"), "old").unwrap();
    let st = extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Overwrite,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(st.skipped, 0);
    assert_eq!(fs::read(dest.join("f.txt")).unwrap(), b"content-v2");

    // -- rename: existing kept, new copy gets " (1)" --
    let dest = root.join("rename");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("f.txt"), "first").unwrap();
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Rename,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(fs::read(dest.join("f.txt")).unwrap(), b"first");
    assert_eq!(fs::read(dest.join("f (1).txt")).unwrap(), b"content-v2");

    // -- overwrite where a DIRECTORY occupies the target: clear error --
    let dest = root.join("dir-block");
    fs::create_dir_all(dest.join("f.txt")).unwrap();
    let err = extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            conflict: wispzip_core::Conflict::Overwrite,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("cannot overwrite directory"),
        "got {err}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn compress_dedups_duplicate_input_names() {
    // two different directories with the same folder name -> second becomes
    // "name (1)" instead of silently merging
    let root = temp_root("dedup");
    let a = root.join("a");
    let b = root.join("b");
    fs::create_dir_all(a.join("data")).unwrap();
    fs::create_dir_all(b.join("data")).unwrap();
    fs::write(a.join("data/x.txt"), b"from-a").unwrap();
    fs::write(b.join("data/x.txt"), b"from-b").unwrap();

    let archive = root.join("out.zip");
    let st = compress(
        &[a.join("data"), b.join("data")],
        &archive,
        &CompressOptions::default(),
        noop_progress,
    )
    .unwrap();
    assert_eq!(st.input_files, 2);

    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"data/x.txt"), "{names:?}");
    assert!(names.contains(&"data (1)/x.txt"), "{names:?}");

    // extraction preserves both copies with distinct content
    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    assert_eq!(fs::read(dest.join("data/x.txt")).unwrap(), b"from-a");
    assert_eq!(fs::read(dest.join("data (1)/x.txt")).unwrap(), b"from-b");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn empty_file_and_deep_nesting_survive_all_archive_formats() {
    // compatibility corner: zero-byte files, empty dirs, very deep paths
    let root = temp_root("edges");
    let src = root.join("tree");
    let deep = src.join("a/b/c/d/e/f/g/h/i/j");
    fs::create_dir_all(&deep).unwrap();
    fs::create_dir_all(src.join("empty-dir")).unwrap();
    fs::write(src.join("zero.bin"), b"").unwrap();
    fs::write(deep.join("leaf.txt"), "deep leaf").unwrap();

    for format in [Format::Zip, Format::SevenZ, Format::Tar] {
        let archive = root.join(format!("e.{}", format.extension()));
        compress(&[src.clone()], &archive, &CompressOptions { format, ..Default::default() }, noop_progress)
            .unwrap_or_else(|e| panic!("{format:?} compress failed: {e}"));
        let dest = root.join(format!("out-{}", format.extension()));
        extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress)
            .unwrap_or_else(|e| panic!("{format:?} extract failed: {e}"));
        assert_eq!(fs::read(dest.join("tree/zero.bin")).unwrap(), b"", "{format:?}");
        assert_eq!(
            fs::read(dest.join("tree/a/b/c/d/e/f/g/h/i/j/leaf.txt")).unwrap(),
            b"deep leaf",
            "{format:?}"
        );
        assert!(dest.join("tree/empty-dir").is_dir(), "{format:?} lost empty dir");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gbk_zip_fixture_roundtrip() {
    // Hand-built GBK zip: what Chinese Windows tools (WinRAR/GoodCompress etc.) tend to produce.
    // produce without the UTF-8 flag — raw GBK bytes in the entry name, a
    // stored (uncompressed) payload, and no UTF-8 flag bit. Assembled
    // byte-by-byte because the zip crate re-encodes names as UTF-8.
    let root = temp_root("gbk-zip");
    let archive = root.join("gbk.zip");
    let payload = b"gbk payload";
    {
        use std::io::Write;
        // "中文报告.txt" in GBK (11 bytes)
        let gbk_name: &[u8] = &[0xD6, 0xD0, 0xCE, 0xC4, 0xB1, 0xA8, 0xB8, 0xE6, b'.', b't', b'x', b't'];
        let crc = crc32(payload);
        let mut f = fs::File::create(&archive).unwrap();
        let mut local = Vec::new();
        local.extend_from_slice(b"PK\x03\x04");
        local.extend_from_slice(&[20, 0]); // version needed
        local.extend_from_slice(&[0, 0]); // flags: no UTF-8 bit
        local.extend_from_slice(&[0, 0]); // method: stored
        local.extend_from_slice(&[0, 0, 0, 0]); // dos time/date
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        local.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        local.extend_from_slice(&(gbk_name.len() as u16).to_le_bytes());
        local.extend_from_slice(&[0, 0]); // extra len
        local.extend_from_slice(gbk_name);
        local.extend_from_slice(payload);
        f.write_all(&local).unwrap();

        let mut central = Vec::new();
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&[20, 0]); // version made by
        central.extend_from_slice(&[20, 0]); // version needed
        central.extend_from_slice(&[0, 0]); // flags
        central.extend_from_slice(&[0, 0]); // method
        central.extend_from_slice(&[0, 0, 0, 0]); // time/date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(gbk_name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0, 0]); // extra field len
        central.extend_from_slice(&[0, 0]); // file comment len
        central.extend_from_slice(&[0, 0]); // disk number start
        central.extend_from_slice(&[0, 0]); // internal file attributes
        central.extend_from_slice(&[0, 0, 0, 0]); // external file attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        central.extend_from_slice(gbk_name);
        f.write_all(&central).unwrap();

        let mut eocd = Vec::new();
        eocd.extend_from_slice(b"PK\x05\x06");
        eocd.extend_from_slice(&[0; 4]); // disk numbers
        eocd.extend_from_slice(&1u16.to_le_bytes()); // entries on this disk
        eocd.extend_from_slice(&1u16.to_le_bytes()); // entries total
        eocd.extend_from_slice(&(central.len() as u32).to_le_bytes());
        eocd.extend_from_slice(&(local.len() as u32).to_le_bytes());
        eocd.extend_from_slice(&[0, 0]); // comment len
        f.write_all(&eocd).unwrap();
    }

    // auto decode -> readable Chinese name
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    assert_eq!(entries[0].name, "中文报告.txt", "{entries:?}");

    // forced utf8 -> replacement chars (documenting the difference)
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Utf8, None).unwrap();
    assert!(entries[0].name.contains('\u{FFFD}'), "{entries:?}");

    // extract with auto: file lands under its proper Chinese name
    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    assert_eq!(
        fs::read(dest.join("中文报告.txt")).unwrap(),
        b"gbk payload"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn large_file_zip64_roundtrip() {
    // ZIP64 path: force >4GB accounting without allocating 4GB by using the
    // large_file flag path — write a ~100MB file and ensure offsets stay sane.
    // (True 4GB+ test is too slow for CI; this exercises the same code path.)
    let root = temp_root("zip64");
    let src = root.join("big.bin");
    fs::write(&src, prng(11, 100 * 1024 * 1024)).unwrap();
    let archive = root.join("big.zip");
    compress(&[src], &archive, &CompressOptions::default(), noop_progress).unwrap();
    let dest = root.join("out");
    extract(&archive, &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    assert_eq!(fs::read(dest.join("big.bin")).unwrap().len(), 100 * 1024 * 1024);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn split_volume_roundtrip() {
    let root = temp_root("split");
    let src = build_tree(&root);
    let expected = walk_files(&src);
    let archive = root.join("out.zip");

    let stats = compress(
        &[src.clone()],
        &archive,
        &CompressOptions { format: Format::Zip, split: Some(4096), ..Default::default() },
        noop_progress,
    )
    .unwrap();
    assert!(!archive.exists(), "single archive must be replaced by volumes");
    assert!(stats.output_path.ends_with("out.zip.001"), "{:?}", stats.output_path);

    let mut vol = 1;
    while root.join(format!("out.zip.{vol:03}")).exists() { vol += 1; }
    assert!(vol >= 3, "expected several volumes, got {vol}");

    // listing through the first volume
    let entries = list_archive(&root.join("out.zip.001"), wispzip_core::NameEncoding::Auto, None).unwrap();
    assert!(entries.iter().any(|e| e.name.contains("a.txt")));

    // extraction through the first volume
    let dest = root.join("out-dir");
    extract(&root.join("out.zip.001"), &ExtractOptions { dest: dest.clone(), ..Default::default() }, noop_progress).unwrap();
    let got = walk_files(&dest);
    assert_eq!(got.len(), expected.len());
    for (exp, got) in expected.iter().zip(got.iter()) {
        assert_eq!(fs::read(&got.0).unwrap(), fs::read(&exp.0).unwrap(), "{:?}" , got.0);
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn split_exact_multiple_has_no_empty_trailing_volume() {
    let root = temp_root("split-exact");
    let src = root.join("payload.bin");
    fs::write(&src, prng(5, 8192)).unwrap();
    compress(
        &[src],
        &root.join("p.7z"),
        &CompressOptions { format: Format::SevenZ, split: Some(4096), ..Default::default() },
        noop_progress,
    )
    .unwrap();
    // 7z of 8KB prng data may exceed 8192 bytes; assert: no zero-byte volume
    let mut vol = 1;
    let mut sizes = Vec::new();
    while root.join(format!("p.7z.{vol:03}")).exists() {
        sizes.push(fs::metadata(root.join(format!("p.7z.{vol:03}"))).unwrap().len());
        vol += 1;
    }
    assert!(!sizes.is_empty());
    assert!(sizes.iter().all(|&s| s > 0), "empty trailing volume: {sizes:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn split_missing_volume_is_reported() {
    let root = temp_root("split-gap");
    let src = root.join("f.bin");
    fs::write(&src, prng(3, 4000)).unwrap();
    compress(
        &[src],
        &root.join("f.zip"),
        &CompressOptions { format: Format::Zip, split: Some(512), ..Default::default() },
        noop_progress,
    )
    .unwrap();
    assert!(root.join("f.zip.002").exists(), "expected multiple volumes");
    fs::remove_file(root.join("f.zip.002")).unwrap();
    let err = extract(
        &root.join("f.zip.001"),
        &ExtractOptions { dest: root.join("out"), ..Default::default() },
        noop_progress,
    )
    .unwrap_err();
    assert!(err.to_string().contains("missing volume"), "got {err}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn non_volume_numeric_suffix_is_ignored() {
    // "photo.002" with no .001 sibling is not a volume set: reported as
    // unknown/unsupported instead of being silently treated as ours
    let root = temp_root("not-vol");
    let f = root.join("photo.002");
    fs::write(&f, b"JPEG-ish bytes, definitely not an archive").unwrap();
    assert!(crate_detect_is_unknown(&f));
    let _ = fs::remove_dir_all(&root);
}

fn crate_detect_is_unknown(f: &std::path::Path) -> bool {
    use wispzip_core::{detect, Classification};
    matches!(
        detect(f),
        Err(WispZipError::UnknownFormat(_)) | Ok((_, Classification::SingleStream))
    )
}

#[test]
fn rar_extracts_plain_fixture() {
    let root = temp_root("rar-plain");
    let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/utf8.rar");
    let dest = root.join("out");
    let st = extract(
        &archive,
        &ExtractOptions { dest: dest.clone(), ..Default::default() },
        noop_progress,
    )
    .unwrap_or_else(|e| panic!("rar extract failed: {e}"));
    assert!(st.files >= 1);
    assert!(walk_files(&dest).len() as u64 == st.files);

    // listing shows the same entries
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    assert_eq!(entries.len() as u64, st.files + 0, "list vs extract count");
}

#[test]
fn rar_encrypted_content_needs_password() {
    let root = temp_root("rar-enc");
    let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/crypted.rar");

    // listing works without a password (plain headers)
    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap();
    assert!(entries.iter().any(|e| e.name == ".gitignore"));

    // no password -> password error
    let err = extract(
        &archive,
        &ExtractOptions { dest: root.join("o1"), ..Default::default() },
        noop_progress,
    )
    .unwrap_err();
    assert!(err.is_password(), "expected password error, got {err}");

    // wrong password -> password error
    let err = extract(
        &archive,
        &ExtractOptions {
            dest: root.join("o2"),
            password: Some("nope".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap_err();
    assert!(err.is_password(), "expected password error, got {err}");

    // correct password -> extracts
    let dest = root.join("o3");
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            password: Some("unrar".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert!(dest.join(".gitignore").is_file());
}

#[test]
fn rar_encrypted_headers_listing_needs_password() {
    let _root = temp_root("rar-hpw");
    let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/comment-hpw-password.rar");

    let err = list_archive(&archive, wispzip_core::NameEncoding::Auto, None).unwrap_err();
    assert!(err.is_password(), "expected password error, got {err}");

    let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, Some("password")).unwrap();
    assert!(entries.iter().any(|e| e.name == ".gitignore"), "{entries:?}");
}

#[test]
fn rar_compression_is_rejected() {
    let root = temp_root("rar-no");
    let src = root.join("f.txt");
    fs::write(&src, "x").unwrap();
    let err = compress(
        &[src],
        &root.join("out.rar"),
        &CompressOptions { format: Format::Rar, ..Default::default() },
        noop_progress,
    )
    .unwrap_err();
    assert!(matches!(err, WispZipError::UnsupportedFormat(_)), "got {err}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn unknown_format_is_reported() {
    let root = temp_root("unknown");
    let f = root.join("mystery.bin");
    let mut file = fs::File::create(&f).unwrap();
    file.write_all(b"not-an-archive-at-all-0123456789").unwrap();
    drop(file);
    let err = list_archive(&f, wispzip_core::NameEncoding::Auto, None).unwrap_err();
    assert!(matches!(err, WispZipError::UnknownFormat(_)));
    let _ = fs::remove_dir_all(&root);
}

/// strip_prefix removes the internal directory structure: extracting files from a subdirectory
/// treats the user-chosen destination as authoritative instead of replaying the archive nesting.
#[test]
fn strip_prefix_flattens_selected_subtree() {
    let root = temp_root("strip-prefix");

    // Build the source tree: Mango/a.txt + Mango/deep/b.txt + top.txt
    let src = root.join("src");
    let mango = src.join("Mango");
    let deep = mango.join("deep");
    fs::create_dir_all(&deep).unwrap();
    fs::write(mango.join("a.txt"), b"alpha").unwrap();
    fs::write(deep.join("b.txt"), b"bravo").unwrap();
    fs::write(src.join("top.txt"), b"top").unwrap();

    // Cover all 7 writable archive formats:
    //   Zip · SevenZ · Tar · TarGz · TarBz2 · TarXz · TarZst
    // RAR is read-only and cannot be produced in this roundtrip test.
    // Gz/Zst/Xz/Bz2 are single-stream formats without directory structure, so strip_prefix does not apply.
    const FORMATS: &[Format] = &[
        Format::Zip,
        Format::SevenZ,
        Format::Tar,
        Format::TarGz,
        Format::TarBz2,
        Format::TarXz,
        Format::TarZst,
    ];

    for format in FORMATS {
        let archive = root.join(format!("sp.{}", format.extension()));
        compress(
            &[src.clone()],
            &archive,
            &CompressOptions { format: *format, ..Default::default() },
            noop_progress,
        )
        .unwrap();

        // Select every file under Mango/ and extract with strip_prefix="src/Mango"
        let dest = root.join(format!("out-{}", format.extension()));
        let stats = extract(
            &archive,
            &ExtractOptions {
                dest: dest.clone(),
                entries: Some(vec![
                    "src/Mango/a.txt".into(),
                    "src/Mango/deep/b.txt".into(),
                ]),
                strip_prefix: Some("src/Mango".into()),
                ..Default::default()
            },
            noop_progress,
        )
        .unwrap();

        assert_eq!(stats.files, 2, "{format:?}: must extract 2 files");
        // After stripping: a.txt lands directly in dest/, b.txt keeps its deep/ subtree
        assert!(dest.join("a.txt").is_file(), "{format:?}: a.txt should be at the dest/ root");
        assert!(
            dest.join("deep").join("b.txt").is_file(),
            "{format:?}: the deep/b.txt subtree should be preserved"
        );
        // The stripped prefix directory itself must not be left behind
        assert!(
            !dest.join("src").exists(),
            "{format:?}: the src/ prefix should not appear"
        );
        assert!(
            !dest.join("Mango").exists(),
            "{format:?}: the Mango/ prefix should not appear"
        );
    }
}

// ---- streaming split (VolumeWriter) ----------------------------------------

/// Concatenate `<stem>.001 .. stem.NNN` back into one byte vector.
fn read_volumes(stem: &Path) -> Vec<u8> {
    let name = stem.file_name().unwrap().to_string_lossy().into_owned();
    let mut out = Vec::new();
    let mut index = 1u32;
    loop {
        let part = stem.with_file_name(format!("{name}.{index:03}"));
        match fs::read(&part) {
            Ok(bytes) => out.extend_from_slice(&bytes),
            Err(_) => break,
        }
        index += 1;
    }
    out
}

fn volume_sizes(stem: &Path) -> Vec<u64> {
    let name = stem.file_name().unwrap().to_string_lossy().into_owned();
    let mut sizes = Vec::new();
    let mut index = 1u32;
    loop {
        let part = stem.with_file_name(format!("{name}.{index:03}"));
        match fs::metadata(&part) {
            Ok(m) => sizes.push(m.len()),
            Err(_) => break,
        }
        index += 1;
    }
    sizes
}

fn staged_parts(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".wispzip-part"))
        .collect()
}

/// The streaming split must not alter a single archive byte: concatenating the
/// volumes has to reproduce the single-file archive exactly, for every format
/// (zip and 7z both seek back into already-written regions near the end).
#[test]
fn split_volumes_are_byte_identical_to_single_archive() {
    const VOL: u64 = 64 * 1024;
    for format in [Format::Zip, Format::SevenZ, Format::Tar, Format::TarGz, Format::TarBz2] {
        let tag = format.extension().replace('.', "-");
        let root = temp_root(&format!("split-bytes-{tag}"));
        let src = root.join("payload.bin");
        fs::write(&src, prng(7, 300 * 1024)).unwrap();

        let single = root.join(format!("single.{}", format.extension()));
        compress(
            &[src.clone()],
            &single,
            &CompressOptions { format, level: 1, ..Default::default() },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{format:?} single compress failed: {e}"));

        let split = root.join(format!("split.{}", format.extension()));
        let stats = compress(
            &[src],
            &split,
            &CompressOptions { format, level: 1, split: Some(VOL), ..Default::default() },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{format:?} split compress failed: {e}"));

        let joined = read_volumes(&split);
        let expected = fs::read(&single).unwrap();
        assert_eq!(
            joined.len(),
            expected.len(),
            "{format:?}: volume set is {} bytes, archive is {}",
            joined.len(),
            expected.len()
        );
        assert!(joined == expected, "{format:?}: concatenated volumes differ from the archive");
        assert_eq!(stats.output_bytes, expected.len() as u64, "{format:?}");

        let sizes = volume_sizes(&split);
        assert!(sizes.len() >= 2, "{format:?}: expected several volumes, got {sizes:?}");
        assert!(
            sizes[..sizes.len() - 1].iter().all(|&s| s == VOL),
            "{format:?}: every volume but the last must be exactly {VOL} bytes: {sizes:?}"
        );
        assert!(sizes[sizes.len() - 1] <= VOL);
        assert!(staged_parts(&root).is_empty(), "{format:?}: staged parts left behind");
        let _ = fs::remove_dir_all(&root);
    }
}

/// A cancelled split run must leave neither volumes nor staging files.
#[test]
fn cancelled_split_leaves_nothing_behind() {
    let root = temp_root("split-cancel");
    let src = root.join("payload.bin");
    fs::write(&src, prng(11, 200 * 1024)).unwrap();
    let out = root.join("c.zip");

    let err = compress(
        &[src],
        &out,
        &CompressOptions { format: Format::Zip, split: Some(16 * 1024), ..Default::default() },
        |_| false,
    )
    .unwrap_err();
    assert!(matches!(err, WispZipError::Cancelled), "{err}");

    assert!(volume_sizes(&out).is_empty(), "no volume may survive a cancel");
    assert!(staged_parts(&root).is_empty(), "staging files must be removed");
    assert!(!out.exists(), "no single archive either");
    let _ = fs::remove_dir_all(&root);
}

// ---- streaming split (VolumeReader) ---------------------------------------

/// Files the old implementation left in the temp directory when reassembling
/// a volume set: one full-size copy of the archive.
fn reassembly_temp_files() -> usize {
    fs::read_dir(std::env::temp_dir())
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("wispzip-vol-"))
                .count()
        })
        .unwrap_or(0)
}

/// Reading a volume set must not depend on a reassembled copy: listing and
/// extraction from `<stem>.001` have to match the single-file archive exactly,
/// for formats whose parsers seek backwards (zip central directory, 7z header
/// at offset 0) as well as for pure streams.
#[test]
fn split_volumes_extract_matches_single_archive() {
    for format in [Format::Zip, Format::SevenZ, Format::TarGz, Format::TarXz] {
        let tag = format.extension().replace('.', "-");
        let root = temp_root(&format!("split-extract-{tag}"));
        let src = build_tree(&root);
        let expected = walk_files(&src);

        let single = root.join(format!("single.{}", format.extension()));
        compress(
            &[src.clone()],
            &single,
            &CompressOptions { format, level: 1, ..Default::default() },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{format:?} single compress failed: {e}"));

        let split = root.join(format!("split.{}", format.extension()));
        compress(
            &[src],
            &split,
            &CompressOptions { format, level: 1, split: Some(64 * 1024), ..Default::default() },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{format:?} split compress failed: {e}"));

        let first = root.join(format!("split.{}.001", format.extension()));
        assert!(first.is_file(), "{format:?}: first volume missing");

        // Listing has to agree entry for entry.
        let from_single = list_archive(&single, wispzip_core::NameEncoding::Auto, None)
            .unwrap_or_else(|e| panic!("{format:?} list single failed: {e}"));
        let from_volumes = list_archive(&first, wispzip_core::NameEncoding::Auto, None)
            .unwrap_or_else(|e| panic!("{format:?} list volumes failed: {e}"));
        let names = |v: &[wispzip_core::EntryInfo]| -> Vec<(String, u64, bool)> {
            v.iter().map(|e| (e.name.clone(), e.size, e.is_dir)).collect()
        };
        assert_eq!(names(&from_single), names(&from_volumes), "{format:?}: listing differs");

        // Extraction has to produce identical bytes and leave no temp copy.
        let dest = root.join("out-dir");
        let temp_before = reassembly_temp_files();
        extract(
            &first,
            &ExtractOptions {
                dest: dest.clone(),
                conflict: wispzip_core::Conflict::Overwrite,
                ..Default::default()
            },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{format:?} extract from volumes failed: {e}"));
        assert_eq!(
            reassembly_temp_files(),
            temp_before,
            "{format:?}: extraction must not reassemble the archive in the temp directory"
        );

        let got = walk_files(&dest);
        assert_eq!(got.len(), expected.len(), "{format:?}");
        for (exp, got) in expected.iter().zip(got.iter()) {
            let rel_exp = exp.0.strip_prefix(&root).unwrap();
            let rel_got = got.0.strip_prefix(&dest).unwrap();
            assert_eq!(rel_exp, rel_got, "{format:?} name mismatch");
            assert_eq!(exp.1, got.1, "{format:?} content mismatch for {rel_exp:?}");
        }
        let _ = fs::remove_dir_all(&root);
    }
}

/// A single-file archive must keep working through the new source abstraction,
/// including the `entries` selection that resolves names before reading.
#[test]
fn single_file_extract_still_selects_entries() {
    let root = temp_root("single-select");
    let src = build_tree(&root);
    let archive = root.join("one.zip");
    compress(
        &[src.clone()],
        &archive,
        &CompressOptions { format: Format::Zip, ..Default::default() },
        noop_progress,
    )
    .unwrap();

    let dest = root.join("picked");
    extract(
        &archive,
        &ExtractOptions {
            dest: dest.clone(),
            entries: Some(vec!["src-tree/nested/a.txt".into()]),
            conflict: wispzip_core::Conflict::Overwrite,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    let got = walk_files(&dest);
    assert_eq!(got.len(), 1, "only the selected entry may be written");
    assert_eq!(got[0].1, fs::read(src.join("nested/a.txt")).unwrap());
    let _ = fs::remove_dir_all(&root);
}
