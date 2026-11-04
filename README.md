# WispZip

**English** | [简体中文](README_CN.md)

A lightweight file archiver — **minimal main screen, drag-and-drop first, right-click ready, with every advanced option folded away**.

The architecture follows a **"one core + multiple thin shells"** pattern: all business logic lives in the Rust core crate, while the desktop app and the CLI are thin shells sharing the same engine.

- Desktop: **Tauri 2 + Vue 3 + TypeScript**
- Core engine: **pure Rust** (`wispzip-core`), streaming end to end — peak RSS stays in the single-digit
  MB range on a 4.5 GiB file and does not grow with file size (measured, see [Performance](#performance))

## Download & Install

Prebuilt packages (dmg / msi / deb / AppImage) are attached to [Releases](https://github.com/hakulat2023/wispzip/releases).

**macOS first-launch note:** the app is not notarized by Apple (no developer account), so a dmg / zip downloaded from the internet carries the quarantine attribute. The first launch may report **"damaged and can't be opened"** or **"cannot verify the developer"**. Either fix works:

```bash
# Recommended: clear the quarantine attribute and it opens normally
xattr -cr /Applications/WispZip.app
```

Or **right-click WispZip.app → Open** (don't double-click). This is a one-time step; later launches show no prompt. Windows / Linux builds just unzip and run.

## Features

| Format | Compress | Extract | Notes |
|---|---|---|---|
| zip | ✅ | ✅ | Deflate 0–9; AES-256 password; ZIP64; auto-detects UTF-8 / GBK filenames |
| tar | ✅ | ✅ | Preserves permissions / mtime |
| tar.gz / tar.bz2 / tar.xz / tar.zst | ✅ | ✅ | Streaming pipeline, live progress reporting |
| gz / zst / xz / bz2 | ✅ | ✅ | Single-file stream compression (xz for ratio, bz2 for balance); keeps the original name minus the suffix |
| 7z | ✅ | ✅ | LZMA2 0–9; AES-256 (content-encrypted, header listable without a password); preserves Unix permission bits; supports AES-256 encrypted archives; `-mhe=on` header-encrypted archives can be previewed with a password (`wispzip l -p`) |
| rar | ➖ | ✅ | Extracts rar4 / rar5 (including encrypted, via the `unrar` library); does not write the proprietary format |
| Split volumes | ✅ | ✅ | `--split 100M` produces byte-level `.001/.002/…` volumes, written **and** read as a stream — no full-size temp archive on either side; works with any inner format |

General capabilities:

- **Content preview** — list entries (name / size / time) without extracting, and pick a subset to extract
- **Progress & cancel** — entry-level and stream-level checkpoints; both the desktop app and the CLI can interrupt immediately
- **Automatic multithreading** — zip extraction derives its concurrency from entry count / total size / CPU cores / system load (enabled at ≥16 entries and ≥1 MB total, falling back to single-threaded when the system is busy), scheduling large files first; tar / 7z / rar stay serial due to format and decoder constraints
- **Safety** — zip-slip protection (rejects `../`, absolute paths, drive letters); every write goes through path validation first
- **Name conflicts** — skip / overwrite / auto-rename as `name (1).ext`
- **GBK filename compatibility** — zips packed by Chinese Windows tools won't garble
- **Split archives, streamed on both sides** — `--split 100M` yields `backup.7z.001/.002/…`. Compression
  maps the archive's logical byte stream straight onto the volume files, and extraction reads back across
  the set through one seekable reader, so neither direction ever materialises a full-size copy. Peak disk
  is ~1x the archive instead of 2x, and nothing is left behind in the temp directory. The inner format is
  detected from the logical name (`backup.tar.gz.001` → tar.gz, not a bare gz stream), and missing or
  out-of-order volumes fail with a clear error. RAR is the one exception: the UnRAR library only accepts a
  real file path, so RAR volume sets are still concatenated before extraction
- **Desktop integration**:
  - Floating main window: drop regular files → compress confirmation; drop an archive → extract confirmation. The confirmation page only asks *what to name it / where to put it* — format, level, password and split volumes all live in the collapsed "Advanced" section
  - The running page supports **pause / resume / cancel**, showing progress, speed and ETA; the done page reports the compression ratio and offers "Open folder"
  - Recent-jobs list (stored locally); click to reveal the item in the file manager
  - System tray (closing the window keeps the app resident; the tray menu shows / quits)
  - **Double-click an associated file** (.zip / .7z / .rar…) opens the extract confirmation directly (macOS Finder / Windows)
  - **macOS Finder Quick Actions** — installation registers "Compress to ZIP" and "Extract" (Finder right-click → Quick Actions / Services; if missing, enable them under System Settings → Extensions → Finder, or use in-app Settings → Finder menu → Reinstall)
  - **Windows context menu** (registered automatically by the NSIS installer): "Compress to ZIP" on files / folders, "Extract to current folder" on archives
- CLI shortcuts: `wispzip c <file>` writes `<file>.zip` when `-o` is omitted; `wispzip x <archive> --here` extracts into the current folder

## Performance

Measured on macOS aarch64 with a release build, using `/usr/bin/time -l` for peak RSS and a `du` poller
for peak disk — these are observations, not estimates.

| Workload | Peak RSS | Peak disk |
|---|---|---|
| zip, 4.5 GiB, Deflate level 1 — compress / extract | 3.2 MB / 3.0 MB | — |
| tar.gz, 4.5 GiB — compress / extract | 3.1 MB / 2.7 MB | — |
| 7z, 4.5 GiB, LZMA2 level 6 — compress / extract | 95 MB / 11 MB | — |
| 1.2 GB → 400 MB × 4 zip, streaming split | 3.2 MB | **1270 MB** (was 2533 MB before streaming) |
| the same set extracted from `.001`, no reassembly | 6.2 MB | 2459 MB = 1266 MB volumes + 1193 MB output |

Memory does not scale with the input: the 7z number is the LZMA2 dictionary for that level, a fixed cost.
Only the top end of xz / zstd asks for real memory (xz `-9` ≈ 611 MB, zstd `-22` ≈ 836 MB, zstd `-19` ≈ 92 MB),
which is a property of the codec rather than of the file being compressed.

## Project layout

```
wispzip/
├── Cargo.toml               # Rust workspace (release: lto + opt-level=s + strip)
├── crates/
│   ├── wispzip-core/        # Core engine: orchestration, formats, progress, cancel, safety
│   │   ├── src/source.rs    # ArchiveSource: one file or a volume set behind a single Read + Seek
│   │   └── src/formats/     # One module per format (zip/tar/streams/sevenz/rar),
│   │                        # each exposing write/extract/list — adding a format = adding a module
│   └── wispzip-cli/         # CLI shell (clap)
├── src-tauri/               # Tauri 2 desktop shell: commands + progress events + task cancel
└── apps/desktop/            # Vue 3 frontend (the single UI)
```

## Development

Requires Rust (stable), Node 20+, and pnpm.

```bash
pnpm install          # frontend dependencies
pnpm tauri dev        # desktop dev mode (vite + cargo running together)
cargo test -p wispzip-core   # core engine tests (104)

# Three test layers:
#   unit tests (inline in src/)  — sniffing, path safety, volumes, progress, time conversion
#   roundtrip.rs                 — full format roundtrips / passwords / conflict matrix / zip-slip / GBK filenames
#   interop.rs                   — bidirectional interop with real tools (7z/xz/bzip2/gzip/tar);
#                                  auto-skips when a tool isn't installed, so it never blocks CI
```

## Build

```bash
pnpm tauri build      # .app/.dmg (macOS), NSIS/MSI (Windows), deb/AppImage (Linux)
cargo build -p wispzip-cli --release   # standalone CLI binary
```

> If DMG packaging fails on macOS because the "Finder automation" permission prompt wasn't granted,
> build just the .app with `pnpm tauri build --bundles app` (CI is unaffected).

The release profile enables `lto + codegen-units=1 + opt-level=s + strip`;
the measured main binary on macOS aarch64 is about **5.7 MB** (the GUI binary embeds the CLI logic, so both share one engine).

The icon assets are committed as-is; `src-tauri/icons/source.svg` is the vector source of truth.
The generator script is kept out of the repository (see `.gitignore`).

## CLI usage

```bash
wispzip compress -o out.tar.gz src/ file.txt   # format inferred from the extension
wispzip c -o backup.zip -l 9 -p secret docs/   # zip with AES-256 encryption
wispzip list backup.zip                        # preview contents
wispzip extract backup.zip -o /target/dir      # extract
wispzip x legacy.zip --encoding gbk            # legacy GBK filenames
wispzip x big.7z --entry dir/file.txt          # extract selected entries only
wispzip c -o backup.7z --split 100M data/      # 100 MB volumes: backup.7z.001, .002, …
wispzip x backup.7z.001                        # extract a split archive (streamed, no reassembly)
wispzip l backup.rar                           # read-only rar preview
wispzip x backup.rar -p secret                 # extract with a password
```

## License

MIT — see [LICENSE](LICENSE).

### Third-party notices

`wispzip-core` links the [`unrar`](https://crates.io/crates/unrar) crate, which bundles RARlab's UnRAR
source. UnRAR is **not** OSI-licensed: it may be used to extract RAR archives, but must not be used to
build a RAR compressor. All other code in this repository is MIT-licensed.
