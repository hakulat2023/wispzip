use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Marker stored inside an [`io::Error`] when a job is cancelled mid-stream.
/// Wrappers raise it so the whole encoder/decoder pipeline unwinds promptly;
/// call sites map it back to [`WispZipError::Cancelled`].
#[derive(Debug)]
pub(crate) struct CancelledIo;

impl std::fmt::Display for CancelledIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("wispzip: operation cancelled")
    }
}

impl std::error::Error for CancelledIo {}

#[derive(Debug, Error)]
pub enum WispZipError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("unrecognized archive: {0}")]
    UnknownFormat(PathBuf),
    #[error("password required or incorrect: {0}")]
    PasswordRequired(String),
    #[error("unsafe entry path rejected (possible zip-slip): {0}")]
    UnsafeEntry(String),
    #[error("io error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = WispZipError> = std::result::Result<T, E>;

impl WispZipError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, WispZipError::Cancelled)
    }

    pub fn is_password(&self) -> bool {
        matches!(self, WispZipError::PasswordRequired(_))
    }

    pub fn is_unsafe_entry(&self) -> bool {
        matches!(self, WispZipError::UnsafeEntry(_))
    }
}

pub(crate) fn cancel_io_error() -> io::Error {
    io::Error::new(io::ErrorKind::Other, CancelledIo)
}

pub(crate) fn is_cancel_io(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|r| r.is::<CancelledIo>())
}

pub(crate) fn io_err(e: io::Error, path: &Path) -> WispZipError {
    if is_cancel_io(&e) {
        WispZipError::Cancelled
    } else {
        WispZipError::Io {
            path: path.to_path_buf(),
            source: e,
        }
    }
}
