//! Where the archive bytes come from.
//!
//! Formats take an [`ArchiveSource`] rather than a `&Path`, so a
//! `<stem>.001/.002/…` volume set is decoded straight through
//! [`VolumeReader`] instead of being concatenated into a full-size temporary
//! copy first. For a plain archive the source is just the file itself, so the
//! two cases share one code path.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use crate::error::{io_err, Result};
use crate::volumes::{self, VolumeReader, VolumeSet};

/// What the parsers need: sequential reads plus random access, usable from a
/// worker thread (the zip extractor opens one reader per thread).
pub(crate) trait ReadSeek: io::Read + io::Seek + Send {}
impl<T: io::Read + io::Seek + Send> ReadSeek for T {}

#[derive(Debug, Clone)]
pub(crate) enum ArchiveSource {
    /// A regular archive file on disk.
    File { path: PathBuf, len: u64 },
    /// A contiguous `<stem>.001 .. <stem>.NNN` volume set.
    Volumes(VolumeSet),
}

impl ArchiveSource {
    pub(crate) fn from_path(path: &Path) -> Result<Self> {
        let len = std::fs::metadata(path).map_err(|e| io_err(e, path))?.len();
        Ok(Self::File {
            path: path.to_path_buf(),
            len,
        })
    }

    pub(crate) fn from_volumes(set: VolumeSet) -> Self {
        Self::Volumes(set)
    }

    /// Length of the archive byte stream (sum of all volumes).
    pub(crate) fn len(&self) -> u64 {
        match self {
            Self::File { len, .. } => *len,
            Self::Volumes(set) => set.total_bytes,
        }
    }

    /// The name the archive has on disk — `.001` for a volume set. Used for
    /// error messages so the user sees a path they can act on.
    pub(crate) fn display_path(&self) -> PathBuf {
        match self {
            Self::File { path, .. } => path.clone(),
            Self::Volumes(set) => volumes::volume_path(&set.stem, 1),
        }
    }

    /// The logical archive path, with the volume suffix removed:
    /// `data.zip.001` -> `data.zip`. Used to derive output names.
    pub(crate) fn logical_path(&self) -> PathBuf {
        match self {
            Self::File { path, .. } => path.clone(),
            Self::Volumes(set) => set.stem.clone(),
        }
    }

    /// Open a fresh reader at byte 0. Cheap: a volume set opens no files until
    /// the first read.
    pub(crate) fn open(&self) -> Result<Box<dyn ReadSeek>> {
        match self {
            Self::File { path, .. } => {
                let file = File::open(path).map_err(|e| io_err(e, path))?;
                Ok(Box::new(BufReader::with_capacity(1 << 20, file)))
            }
            Self::Volumes(set) => Ok(Box::new(VolumeReader::open(set.clone()))),
        }
    }
}
