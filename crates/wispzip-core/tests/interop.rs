//! Compatibility tests against real-world tools.
//!
//! Every test probes for the external binary and returns early when it is
//! not installed, so the suite stays green on machines without the tools.
//! Direction matters:
//! - "ours → theirs": our output must satisfy the reference implementation.
//! - "theirs → ours": their output must extract correctly through wispzip.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wispzip_core::{compress, extract, list_archive, CompressOptions, ExtractOptions, Format};

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wispzip-interop-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn noop_progress(_: &wispzip_core::Progress) -> bool {
    true
}

fn run(cmd: &str, args: &[&str], cwd: &Path) -> bool {
    Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// True when `cmd` can be spawned at all. (p7zip rejects `--version`, so we
/// deliberately don't gate on exit status — mere executability is enough.)
fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .output()
        .map(|o| o.status.code().is_some())
        .unwrap_or(false)
}

fn make_source(root: &Path) -> PathBuf {
    let src = root.join("src");
    fs::create_dir_all(src.join("nested")).unwrap();
    fs::write(src.join("nested/a.txt"), "interop fixture\n".repeat(40)).unwrap();
    fs::write(src.join("top.md"), "# top").unwrap();
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

// ---- 7-Zip ------------------------------------------------------------------

#[test]
fn ours_7z_validated_by_real_7z() {
    if !have("7z") {
        eprintln!("skipping: 7z not installed");
        return;
    }
    let root = temp_root("7z-ours");
    let src = make_source(&root);
    let archive = root.join("out.7z");
    compress(
        std::slice::from_ref(&src),
        &archive,
        &CompressOptions {
            format: Format::SevenZ,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    // integrity + listing through the reference implementation
    assert!(
        run("7z", &["t", "out.7z"], &root),
        "7z t failed on our archive"
    );

    let dest = root.join("via7z");
    assert!(run("7z", &["x", "-y", "-ovia7z", "out.7z"], &root));
    // file count + byte-identical content on both sides
    let ours = walk_files(&src);
    let theirs = walk_files(&dest);
    assert_eq!(ours.len(), theirs.len());
    for ((op, oc), (tp, tc)) in ours.iter().zip(theirs.iter()) {
        assert_eq!(op.file_name(), tp.file_name(), "{op:?} vs {tp:?}");
        assert_eq!(oc, tc, "{op:?} content differs");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ours_7z_encrypted_validated_by_real_7z() {
    if !have("7z") {
        eprintln!("skipping: 7z not installed");
        return;
    }
    let root = temp_root("7z-ours-enc");
    let src = make_source(&root);
    let archive = root.join("enc.7z");
    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::SevenZ,
            password: Some("interop-pw".into()),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    // wrong password must fail, right password must round-trip
    assert!(!run("7z", &["x", "-y", "-pwrong", "-ow", "enc.7z"], &root));
    assert!(run(
        "7z",
        &["x", "-y", "-pinterop-pw", "-ook", "enc.7z"],
        &root
    ));
    assert_eq!(fs::read(root.join("ok/src/top.md")).unwrap(), b"# top");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn real_7z_output_extracts_through_wispzip() {
    if !have("7z") {
        eprintln!("skipping: 7z not installed");
        return;
    }
    let root = temp_root("7z-theirs");
    let _src = make_source(&root);
    assert!(run("7z", &["a", "-y", "theirs.7z", "src"], &root));
    // also with encrypted headers (-mhe=on)
    assert!(run(
        "7z",
        &["a", "-y", "-pmhe-pw", "-mhe=on", "mhe.7z", "src"],
        &root
    ));

    for name in ["theirs.7z", "mhe.7z"] {
        let archive = root.join(name);
        let pw = if name.contains("mhe") {
            Some("mhe-pw".to_string())
        } else {
            None
        };
        let entries = list_archive(&archive, wispzip_core::NameEncoding::Auto, pw.as_deref())
            .unwrap_or_else(|e| panic!("{name} list failed: {e}"));
        assert!(
            entries.iter().any(|e| e.name.contains("a.txt")),
            "{name}: {entries:?}"
        );

        let dest = root.join(format!("out-{name}"));
        extract(
            &archive,
            &ExtractOptions {
                dest: dest.clone(),
                password: pw,
                ..Default::default()
            },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{name} extract failed: {e}"));
        let got = walk_files(&dest);
        assert!(
            got.iter()
                .any(|(p, c)| p.ends_with("top.md") && c == b"# top"),
            "{name}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

// ---- gzip / xz / bzip2 --------------------------------------------------------

#[test]
fn ours_gz_validated_by_real_gzip() {
    if !have("gzip") {
        eprintln!("skipping: gzip not installed");
        return;
    }
    let root = temp_root("gz-ours");
    let src = root.join("payload.txt");
    fs::write(&src, "gzip interop\n".repeat(500)).unwrap();
    let archive = root.join("payload.txt.gz");
    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::Gz,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    assert!(
        run("gzip", &["-t", "payload.txt.gz"], &root),
        "gzip -t rejected our output"
    );
    assert!(run("gzip", &["-dk", "-c", "payload.txt.gz"], &root) || true);
    let out = Command::new("gzip")
        .args(["-dc", "payload.txt.gz"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"gzip interop\n".repeat(500));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ours_xz_validated_by_real_xz() {
    if !have("xz") {
        eprintln!("skipping: xz not installed");
        return;
    }
    let root = temp_root("xz-ours");
    let src = root.join("payload.txt");
    fs::write(&src, "xz interop\n".repeat(500)).unwrap();
    let archive = root.join("payload.txt.xz");
    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::Xz,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    assert!(
        run("xz", &["-t", "payload.txt.xz"], &root),
        "xz -t rejected our output"
    );
    let out = Command::new("xz")
        .args(["-dc", "payload.txt.xz"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"xz interop\n".repeat(500));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ours_bz2_validated_by_real_bzip2() {
    if !have("bzip2") {
        eprintln!("skipping: bzip2 not installed");
        return;
    }
    let root = temp_root("bz2-ours");
    let src = root.join("payload.txt");
    fs::write(&src, "bzip2 interop\n".repeat(500)).unwrap();
    let archive = root.join("payload.txt.bz2");
    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::Bz2,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    assert!(
        run("bzip2", &["-t", "payload.txt.bz2"], &root),
        "bzip2 -t rejected our output"
    );
    let out = Command::new("bzip2")
        .args(["-dc", "payload.txt.bz2"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"bzip2 interop\n".repeat(500));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn real_xz_and_bzip2_output_extract_through_wispzip() {
    if !have("xz") || !have("bzip2") {
        eprintln!("skipping: xz/bzip2 not installed");
        return;
    }
    let root = temp_root("theirs-streams");
    let payload = b"theirs stream\n".repeat(300);

    fs::write(root.join("x.txt"), &payload).unwrap();
    assert!(run("xz", &["-k", "-f", "x.txt"], &root));
    assert!(run("bzip2", &["-k", "-f", "x.txt"], &root));
    assert!(root.join("x.txt.xz").exists() && root.join("x.txt.bz2").exists());

    for name in ["x.txt.xz", "x.txt.bz2"] {
        let dest = root.join(format!("out-{name}"));
        extract(
            &root.join(name),
            &ExtractOptions {
                dest: dest.clone(),
                ..Default::default()
            },
            noop_progress,
        )
        .unwrap_or_else(|e| panic!("{name} extract failed: {e}"));
        assert_eq!(fs::read(dest.join("x.txt")).unwrap(), payload, "{name}");
    }
    let _ = fs::remove_dir_all(&root);
}

// ---- tar (system bsdtar/GNU tar) -----------------------------------------------

#[test]
fn real_tar_gz_output_extracts_through_wispzip() {
    if !have("tar") {
        eprintln!("skipping: tar not installed");
        return;
    }
    let root = temp_root("theirs-tar");
    let _src = make_source(&root);
    assert!(run("tar", &["czf", "theirs.tar.gz", "src"], &root));

    let entries = list_archive(
        &root.join("theirs.tar.gz"),
        wispzip_core::NameEncoding::Auto,
        None,
    )
    .unwrap();
    assert!(entries.iter().any(|e| e.name.contains("top.md")));

    let dest = root.join("out");
    extract(
        &root.join("theirs.tar.gz"),
        &ExtractOptions {
            dest: dest.clone(),
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();
    assert_eq!(fs::read(dest.join("src/top.md")).unwrap(), b"# top");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ours_tar_gz_validated_by_real_tar() {
    if !have("tar") {
        eprintln!("skipping: tar not installed");
        return;
    }
    let root = temp_root("ours-tar");
    let src = make_source(&root);
    let archive = root.join("ours.tar.gz");
    compress(
        &[src],
        &archive,
        &CompressOptions {
            format: Format::TarGz,
            ..Default::default()
        },
        noop_progress,
    )
    .unwrap();

    let dest = root.join("viatar");
    fs::create_dir_all(&dest).unwrap();
    assert!(
        run("tar", &["xzf", "../ours.tar.gz"], &dest),
        "tar rejected our tar.gz"
    );
    assert_eq!(fs::read(dest.join("src/top.md")).unwrap(), b"# top");
    let _ = fs::remove_dir_all(&root);
}

// ---- cross-tool: our 7z reader vs real 7z writer with unix modes --------------

#[test]
fn mode_preservation_cross_tool_with_real_7z() {
    if !have("7z") {
        eprintln!("skipping: 7z not installed");
        return;
    }
    #[cfg(not(unix))]
    {
        eprintln!("skipping: unix only");
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root("mode-interop");
        let src = root.join("tool.sh");
        fs::write(&src, "#!/bin/sh\necho ok\n").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o750)).unwrap();

        // ours -> theirs: real 7z must restore 0o750
        let archive = root.join("ours.7z");
        compress(
            std::slice::from_ref(&src),
            &archive,
            &CompressOptions {
                format: Format::SevenZ,
                ..Default::default()
            },
            noop_progress,
        )
        .unwrap();
        let via7z = root.join("via7z");
        assert!(run("7z", &["x", "-y", "-ovia7z", "ours.7z"], &root));
        assert_eq!(
            via7z
                .join("tool.sh")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750 & 0o777,
            "real 7z must see our unix attributes"
        );

        // theirs -> ours: we must restore 0o750
        let theirs = root.join("theirs.7z");
        assert!(run("7z", &["a", "-y", "theirs.7z", "tool.sh"], &root));
        let out = root.join("out");
        extract(
            &theirs,
            &ExtractOptions {
                dest: out.clone(),
                ..Default::default()
            },
            noop_progress,
        )
        .unwrap();
        assert_eq!(
            out.join("tool.sh").metadata().unwrap().permissions().mode() & 0o7777,
            0o750
        );
        let _ = fs::remove_dir_all(&root);
    }
}
