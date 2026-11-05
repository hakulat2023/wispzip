# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-05

### Added

- A [GitHub Pages landing page](https://hakulat2023.github.io/wispzip/) with
  Chinese / English copy and localized app screenshots. The download button
  reads the Releases API at runtime, so it points at the latest published
  build without a page edit.
- `WISPZIP_TAR_WORKERS=1..8` overrides the number of tar write workers.
- This changelog.

### Fixed

- macOS: Finder "Open With" and double-clicking an associated archive now open
  the extract confirmation dialog directly instead of landing on the home
  screen. Two causes were involved: `App.vue` called `invoke` without
  importing it, so a runtime `ReferenceError` aborted `onMounted` and the
  cached cold-start path was never claimed; and the single-instance plugin
  swallowed the Finder Apple Event.
- De-duplicate the cold-start re-emit of the same opened path within a 2s
  window.

### Changed

- tar extraction: the stream is still decoded sequentially, but entries
  ≤64 KiB are written by a bounded worker pool (64-entry queue, ~4 MiB).
  Larger entries keep the O(1)-memory streaming path. Duplicate names are
  sharded to one worker so skip / overwrite / auto-rename semantics are
  preserved.
- Parent directory creation is cached per extraction instead of repeating
  `create_dir_all` for every file.
- Progress reporting in tar / 7z / rar is throttled (80 ms / 1 MiB) instead of
  forcing one webview event per file.
- The tar worker default is `min(cores, 4)`; the 1-minute load average is no
  longer used to throttle I/O-bound workers.
- Version bumped to 0.2.0 across the Rust workspace, the desktop package and
  the Tauri config.

### Performance

Benchmarked with the release CLI on the shared `wispzip-core` engine, macOS
aarch64, cold output directory. Sample: 490 MiB unpacked source tarball with
13,572 files (61.8% ≤ 4 KiB) plus one 198 MB file.

| | Time |
|---|---|
| WispZip 0.1.0 | 3.45 s |
| system bsdtar | 2.74 s |
| **WispZip 0.2.0, 4 workers (default)** | **1.62 s** |

These are CLI figures; the GUI was not timed.

## [0.1.0] - 2026-11-04

First public release.

- zip / 7z / tar(.gz/.bz2/.xz/.zst) / gz / zst / xz / bz2 compression, rar
  extraction
- Streaming engine: peak RSS stays flat at GB-scale inputs
- Split volumes (`<archive>.001/.002/...`) streamed on both compress and
  extract, so neither side materialises a full-size temp archive
- AES-256 for zip and 7z, encrypted-header 7z preview

[0.2.0]: https://github.com/hakulat2023/wispzip/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hakulat2023/wispzip/releases/tag/v0.1.0
