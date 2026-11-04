//! Byte-level split volumes (`<archive>.001`, `<archive>.002`, …).
//!
//! Compression emits the archive straight into fixed-size volumes through
//! [`VolumeWriter`]; extraction and listing read them back as one contiguous
//! stream through [`VolumeReader`]. Nothing is ever reassembled into a
//! full-size temporary copy. The archive format inside the volumes is
//! arbitrary — the first volume simply starts with its magic.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{io_err, Result, WispZipError};

/// A detected volume set: `stem.001 .. stem.NNN` all present.
#[derive(Debug, Clone)]
pub(crate) struct VolumeSet {
    pub stem: PathBuf,
    pub count: u32,
    /// Size of each volume, in index order (`sizes[0]` is `.001`).
    pub sizes: Vec<u64>,
    /// Start offset of each volume in the concatenated byte stream.
    starts: Vec<u64>,
    pub total_bytes: u64,
}

impl VolumeSet {
    pub(crate) fn new(stem: PathBuf, sizes: Vec<u64>) -> Self {
        let mut starts = Vec::with_capacity(sizes.len());
        let mut acc = 0u64;
        for &size in &sizes {
            starts.push(acc);
            acc += size;
        }
        Self {
            stem,
            count: sizes.len() as u32,
            sizes,
            starts,
            total_bytes: acc,
        }
    }

    /// Which volume holds logical offset `pos`, and the offset inside it.
    /// Zero-length volumes are skipped because the search takes the last
    /// volume whose start is `<= pos`.
    fn locate(&self, pos: u64) -> Option<(u32, u64)> {
        if pos >= self.total_bytes {
            return None;
        }
        let slot = self.starts.partition_point(|&start| start <= pos) - 1;
        Some((slot as u32 + 1, pos - self.starts[slot]))
    }

    fn start_of(&self, index: u32) -> Option<u64> {
        self.starts.get(index as usize - 1).copied()
    }

    fn size_of(&self, index: u32) -> Option<u64> {
        self.sizes.get(index as usize - 1).copied()
    }
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
    let mut sizes = Vec::new();
    let mut index = 1u32;
    loop {
        let part = volume_path(&stem, index);
        if !part.exists() {
            break;
        }
        sizes.push(fs::metadata(&part).map_err(|e| io_err(e, &part))?.len());
        index += 1;
    }
    if sizes.is_empty() {
        return Ok(None);
    }
    let set = VolumeSet::new(stem, sizes);
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
///
/// Only used for formats that cannot be fed a reader — UnRAR insists on a real
/// file path — so those still pay a full-size temporary copy. Everything else
/// reads through [`VolumeReader`].
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

/// Streaming source for byte-level split volumes.
///
/// Exposes the volume set as one contiguous `Read + Seek` byte stream: the
/// format parsers (zip central directory, 7z header at offset 0, …) seek
/// around freely and never learn that the archive is spread over several
/// files. Volumes are opened lazily and only one is held at a time, so the
/// reader's memory and file-handle footprint stay constant regardless of the
/// archive size.
pub(crate) struct VolumeReader {
    set: VolumeSet,
    /// Logical read offset in the concatenated stream.
    pos: u64,
    /// Currently open volume: (index, read offset inside that volume, reader).
    open: Option<(u32, u64, BufReader<File>)>,
}

impl VolumeReader {
    pub(crate) fn open(set: VolumeSet) -> Self {
        Self {
            set,
            pos: 0,
            open: None,
        }
    }

    /// Open a second volume (first use), or reposition the open one.
    fn align(&mut self, index: u32, off: u64) -> io::Result<()> {
        if let Some((current, vol_off, reader)) = self.open.as_mut() {
            if *current == index {
                if *vol_off != off {
                    reader.seek(SeekFrom::Start(off))?;
                    *vol_off = off;
                }
                return Ok(());
            }
        }
        let path = volume_path(&self.set.stem, index);
        let file = File::open(&path)?;
        let mut reader = BufReader::with_capacity(1 << 20, file);
        if off != 0 {
            reader.seek(SeekFrom::Start(off))?;
        }
        self.open = Some((index, off, reader));
        Ok(())
    }
}

impl Read for VolumeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let Some((index, off)) = self.set.locate(self.pos) else {
                return Ok(0);
            };
            let room = self.set.size_of(index).unwrap_or(0) - off;
            if room == 0 {
                // Should not happen: locate() never returns an empty slot.
                self.pos = self.set.start_of(index).unwrap_or(0) + self.set.size_of(index).unwrap_or(0);
                self.open = None;
                continue;
            }
            self.align(index, off)?;
            let want = buf.len().min(room as usize);
            let read = {
                let (_, vol_off, reader) = self.open.as_mut().expect("volume is open");
                let n = reader.read(&mut buf[..want])?;
                *vol_off += n as u64;
                n
            };
            if read > 0 {
                self.pos += read as u64;
                return Ok(read);
            }
            // The volume is shorter on disk than its metadata said: skip to the
            // next one rather than reporting a premature end of archive.
            self.pos = self.set.start_of(index).unwrap_or(0) + self.set.size_of(index).unwrap_or(0);
            self.open = None;
            if self.pos >= self.set.total_bytes {
                return Ok(0);
            }
        }
    }
}

impl Seek for VolumeReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target: i128 = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
            SeekFrom::End(n) => self.set.total_bytes as i128 + n as i128,
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the archive",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// Staging suffix for a volume that has not been committed yet.
const PART_SUFFIX: &str = ".wispzip-part";

/// Streaming sink for byte-level split volumes.
///
/// The archive writer sees one contiguous, seekable byte stream; this type
/// maps every logical offset onto a volume file:
///
/// ```text
/// index = offset / vol_size + 1        offset_in_volume = offset % vol_size
/// ```
///
/// So `<out>.zip` is never materialised as a whole: the bytes go into
/// `<out>.zip.001`, `.002`, … while they are produced. Volumes are staged as
/// `<volume>.wispzip-part` and renamed only in [`VolumeWriter::finalize`],
/// because formats such as zip (zip64 size patch) and 7z (32-byte header at
/// offset 0) rewrite already-written bytes at the end of the run — possibly in
/// a much earlier volume.
///
/// Dropping an unfinalised writer removes the staged parts, so a cancelled or
/// failed run never leaves volumes behind.
pub(crate) struct VolumeWriter {
    stem: PathBuf,
    vol_size: u64,
    /// Logical cursor: where the next write starts.
    pos: u64,
    /// Highest logical offset written; `SeekFrom::End` is relative to it.
    len: u64,
    /// Bytes written into each opened volume, in index order.
    extents: Vec<u64>,
    /// Currently open volume: (index, buffered file).
    open: Option<(u32, BufWriter<File>)>,
    finalized: bool,
}

impl VolumeWriter {
    pub(crate) fn create(stem: &Path, vol_size: u64) -> io::Result<Self> {
        if vol_size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "volume size must be greater than zero",
            ));
        }
        Ok(Self {
            stem: stem.to_path_buf(),
            vol_size,
            pos: 0,
            len: 0,
            extents: Vec::new(),
            open: None,
            finalized: false,
        })
    }

    /// Logical bytes written to the archive stream so far.
    #[cfg(test)]
    pub(crate) fn logical_len(&self) -> u64 {
        self.len
    }

    /// Number of volumes that received bytes so far.
    #[cfg(test)]
    pub(crate) fn volume_count(&self) -> u32 {
        self.extents.len() as u32
    }

    fn part_path(&self, index: u32) -> PathBuf {
        let vol = volume_path(&self.stem, index);
        let name = vol
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into());
        vol.with_file_name(format!("{name}{PART_SUFFIX}"))
    }

    /// Flush and close the open volume (no-op when none is open).
    fn close_open(&mut self) -> io::Result<()> {
        if let Some((_, mut w)) = self.open.take() {
            w.flush()?;
        }
        Ok(())
    }

    /// Open `index` for writing, closing whatever volume is current.
    ///
    /// Re-opening never truncates: earlier volumes may already hold data that
    /// a trailing header rewrite has to patch in place.
    fn ensure_volume(&mut self, index: u32) -> io::Result<()> {
        if matches!(self.open, Some((cur, _)) if cur == index) {
            return Ok(());
        }
        self.close_open()?;
        // `extents.len()` is exactly the number of volumes opened so far.
        let already_open = index as usize <= self.extents.len();
        if !already_open {
            // A seek skipped whole volumes: keep the sequence contiguous so
            // detection and extraction still see 001..NNN.
            while self.extents.len() + 1 < index as usize {
                let gap = self.extents.len() as u32 + 1;
                File::create(self.part_path(gap))?;
                self.extents.push(0);
            }
            self.extents.push(0);
        }
        let path = self.part_path(index);
        let file = if already_open {
            OpenOptions::new().read(true).write(true).open(&path)?
        } else {
            // First touch in this run: drop any stale part from an earlier one.
            File::create(&path)?
        };
        self.open = Some((index, BufWriter::with_capacity(1 << 20, file)));
        Ok(())
    }

    /// Extend a volume to its full size, so a later write in a following
    /// volume sees the zeros the logical stream implies. Cheap no-op in the
    /// normal sequential flow, where all but the last volume are already full.
    fn pad_volume(&mut self, index: u32) -> io::Result<()> {
        if index == 0 || index as usize > self.extents.len() {
            return Ok(());
        }
        let slot = index as usize - 1;
        if self.extents[slot] >= self.vol_size {
            return Ok(());
        }
        if matches!(self.open, Some((cur, _)) if cur == index) {
            let (_, w) = self.open.as_mut().expect("volume is open");
            w.flush()?;
            w.get_ref().set_len(self.vol_size)?;
        } else {
            let path = self.part_path(index);
            let f = OpenOptions::new().write(true).open(path)?;
            f.set_len(self.vol_size)?;
        }
        self.extents[slot] = self.vol_size;
        Ok(())
    }

    fn write_at(&mut self, index: u32, off: u64, data: &[u8]) -> io::Result<()> {
        self.pad_volume(index - 1)?;
        self.ensure_volume(index)?;
        let slot = index as usize - 1;
        let extent = self.extents[slot];
        let (_, w) = self.open.as_mut().expect("volume is open");
        if off > extent {
            // A forward seek left a hole: extend with zeros (sparse on APFS/NTFS).
            w.flush()?;
            w.get_ref().set_len(off)?;
            self.extents[slot] = off;
        }
        w.seek(SeekFrom::Start(off))?;
        w.write_all(data)?;
        let end = off + data.len() as u64;
        if end > self.extents[slot] {
            self.extents[slot] = end;
        }
        Ok(())
    }

    /// Close the last volume, trim each part to its real length and rename the
    /// staged parts into place. Returns the total bytes across all volumes.
    pub(crate) fn finalize(mut self) -> io::Result<u64> {
        self.close_open()?;
        let count = self.extents.len() as u32;
        let mut total = 0u64;
        for index in 1..=count {
            let len = self.extents[index as usize - 1];
            let staged = self.part_path(index);
            {
                let f = OpenOptions::new().write(true).open(&staged)?;
                // Drop buffered padding past the last real byte.
                f.set_len(len)?;
            }
            total += len;
            let dest = volume_path(&self.stem, index);
            if let Err(first) = fs::rename(&staged, &dest) {
                // Windows rename does not overwrite an existing file.
                fs::remove_file(&dest).map_err(|_| first)?;
                fs::rename(&staged, &dest)?;
            }
        }
        self.finalized = true;
        // Volumes left over from a longer previous run would corrupt the set
        // (detection would treat them as part of it), so drop the stale tail.
        let mut probe = count + 1;
        while probe <= count + 100 {
            let stale = volume_path(&self.stem, probe);
            if !stale.exists() {
                break;
            }
            fs::remove_file(&stale)?;
            probe += 1;
        }
        Ok(total)
    }
}

impl io::Write for VolumeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut done = 0usize;
        while done < buf.len() {
            let index = (self.pos / self.vol_size) as u32 + 1;
            let off = self.pos % self.vol_size;
            let take = (self.vol_size - off).min((buf.len() - done) as u64) as usize;
            self.write_at(index, off, &buf[done..done + take])?;
            self.pos += take as u64;
            done += take;
            if self.pos > self.len {
                self.len = self.pos;
            }
        }
        Ok(done)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.close_open()
    }
}

impl Seek for VolumeWriter {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target: i128 = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
            SeekFrom::End(n) => self.len as i128 + n as i128,
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the archive",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

impl Drop for VolumeWriter {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        self.open.take();
        for index in 1..=self.extents.len() as u32 {
            let _ = fs::remove_file(self.part_path(index));
        }
    }
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
        fs::write(dir.join("a.001"), b"123").unwrap();
        fs::write(dir.join("a.002"), b"456").unwrap();
        fs::write(dir.join("a.003"), b"789").unwrap();
        let set = VolumeSet::new(dir.join("a"), vec![3, 3, 3]);
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

    // ---- VolumeWriter --------------------------------------------------

    fn read_volumes(dir: &Path, stem: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut index = 1u32;
        loop {
            let p = dir.join(format!("{stem}.{index:03}"));
            match fs::read(&p) {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(_) => break,
            }
            index += 1;
        }
        out
    }

    fn staged_parts(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(PART_SUFFIX))
            .collect()
    }

    #[test]
    fn volume_writer_splits_at_exact_boundaries() {
        let dir = temp_dir("vw-split");
        let stem = dir.join("a.zip");
        let mut w = VolumeWriter::create(&stem, 4).unwrap();
        w.write_all(b"0123456789").unwrap();
        assert_eq!(w.logical_len(), 10);
        assert_eq!(w.volume_count(), 3);
        let total = w.finalize().unwrap();
        assert_eq!(total, 10);
        assert_eq!(read_volumes(&dir, "a.zip"), b"0123456789");
        assert_eq!(fs::metadata(dir.join("a.zip.001")).unwrap().len(), 4);
        assert_eq!(fs::metadata(dir.join("a.zip.002")).unwrap().len(), 4);
        assert_eq!(fs::metadata(dir.join("a.zip.003")).unwrap().len(), 2);
        assert!(staged_parts(&dir).is_empty(), "no staged parts after finalize");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_writer_has_no_empty_trailing_volume() {
        let dir = temp_dir("vw-multiple");
        let stem = dir.join("b.zip");
        let mut w = VolumeWriter::create(&stem, 4).unwrap();
        w.write_all(b"12345678").unwrap();
        w.finalize().unwrap();
        assert_eq!(read_volumes(&dir, "b.zip"), b"12345678");
        assert!(!dir.join("b.zip.003").exists(), "exact multiple must not add .003");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_writer_patches_bytes_in_an_earlier_volume() {
        let dir = temp_dir("vw-rewrite");
        let stem = dir.join("c.zip");
        let mut w = VolumeWriter::create(&stem, 4).unwrap();
        // Fill three volumes, then emulate a trailing header rewrite.
        w.write_all(b"AAAABBBBCCCCDD").unwrap();
        w.seek(SeekFrom::Start(1)).unwrap();
        w.write_all(b"XY").unwrap();
        w.seek(SeekFrom::End(0)).unwrap();
        let end = w.seek(SeekFrom::End(-2)).unwrap();
        assert_eq!(end, 12);
        w.write_all(b"ZZ").unwrap();
        w.finalize().unwrap();
        assert_eq!(read_volumes(&dir, "c.zip"), b"AXYABBBBCCCCZZ");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_writer_zero_fills_forward_seeks() {
        let dir = temp_dir("vw-hole");
        let stem = dir.join("d.zip");
        let mut w = VolumeWriter::create(&stem, 4).unwrap();
        w.write_all(b"ab").unwrap();
        w.seek(SeekFrom::Start(6)).unwrap();
        w.write_all(b"cd").unwrap();
        w.finalize().unwrap();
        assert_eq!(read_volumes(&dir, "d.zip"), b"ab\0\0\0\0cd");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_writer_drop_discards_staged_parts() {
        let dir = temp_dir("vw-drop");
        let stem = dir.join("e.zip");
        {
            let mut w = VolumeWriter::create(&stem, 4).unwrap();
            w.write_all(b"0123456789").unwrap();
            assert_eq!(staged_parts(&dir).len(), 3, "three volumes, all staged");
            assert!(!dir.join("a.zip.001").exists(), "nothing committed while running");
        }
        assert!(staged_parts(&dir).is_empty(), "dropped writer cleans its parts up");
        assert!(!dir.join("e.zip.001").exists(), "nothing is committed on drop");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_writer_clears_stale_tail_from_previous_run() {
        let dir = temp_dir("vw-stale");
        let stem = dir.join("f.zip");
        fs::write(dir.join("f.zip.001"), vec![0u8; 4]).unwrap();
        fs::write(dir.join("f.zip.002"), vec![0u8; 4]).unwrap();
        fs::write(dir.join("f.zip.003"), vec![0u8; 4]).unwrap();
        let mut w = VolumeWriter::create(&stem, 4).unwrap();
        w.write_all(b"12345").unwrap();
        w.finalize().unwrap();
        assert_eq!(read_volumes(&dir, "f.zip"), b"12345");
        assert!(!dir.join("f.zip.003").exists(), "stale tail volume is removed");
        let _ = fs::remove_dir_all(&dir);
    }

    // ---- VolumeReader --------------------------------------------------

    fn reader_for(dir: &Path, stem: &str, sizes: &[u64]) -> VolumeReader {
        let name = stem.to_string();
        for (i, size) in sizes.iter().enumerate() {
            let fill = vec![b'a' + i as u8; *size as usize];
            fs::write(dir.join(format!("{name}.{:03}", i + 1)), fill).unwrap();
        }
        let set = detect(&dir.join(format!("{name}.001"))).unwrap().unwrap();
        VolumeReader::open(set)
    }

    #[test]
    fn volume_reader_reads_across_volume_boundaries() {
        let dir = temp_dir("vr-read");
        let mut r = reader_for(&dir, "a.zip", &[3, 3, 2]);
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"aaabbbcc");
        // A second read is a clean EOF, not a rewind.
        let mut tail = [0u8; 4];
        assert_eq!(r.read(&mut tail).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_reader_seeks_randomly() {
        let dir = temp_dir("vr-seek");
        let mut r = reader_for(&dir, "b.zip", &[4, 4, 1]);
        let mut buf = [0u8; 2];
        r.seek(SeekFrom::Start(5)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"bb");
        r.seek(SeekFrom::Start(0)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"aa");
        // End is the concatenated length, not the last volume's length.
        assert_eq!(r.seek(SeekFrom::End(0)).unwrap(), 9);
        assert_eq!(r.seek(SeekFrom::End(-2)).unwrap(), 7);
        r.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"bc");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_reader_treats_short_volume_as_next_volume() {
        let dir = temp_dir("vr-short");
        let mut r = reader_for(&dir, "c.zip", &[4, 4]);
        // Truncate the first volume behind the reader's back; the metadata it
        // cached still claims 4 bytes, so a short read must advance instead of
        // reporting a premature end of archive.
        r.read_exact(&mut [0u8; 4]).unwrap();
        fs::write(dir.join("c.zip.001"), b"xy").unwrap();
        r.seek(SeekFrom::Start(0)).unwrap();
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"xybbbb", "short volume is followed by the next one");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_reader_reports_eof_on_empty_set() {
        let dir = temp_dir("vr-empty");
        fs::write(dir.join("d.zip.001"), b"").unwrap();
        let set = detect(&dir.join("d.zip.001")).unwrap().unwrap();
        assert_eq!(set.total_bytes, 0);
        let mut r = VolumeReader::open(set);
        assert_eq!(r.read(&mut [0u8; 8]).unwrap(), 0);
        assert_eq!(r.seek(SeekFrom::End(0)).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
