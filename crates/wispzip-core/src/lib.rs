//! wispzip-core: cross-platform archive engine shared by the desktop app and the CLI.
//!
//! Zero UI dependencies. Formats: zip / tar / tar.gz / tar.bz2 / tar.xz / tar.zst /
//! gz / zst / xz / bz2, 7z compression+extraction and rar extraction. Byte-level
//! split volumes, progress reporting, cancellation and zip-slip protection are
//! built in.

pub mod compress;
pub mod error;
pub mod extract;
pub mod format;
pub mod formats;
pub mod listing;
pub mod progress;
pub mod safe;
mod sysload;
mod volumes;

pub use compress::{compress, scan, CompressOptions, CompressStats, ScanStats};
pub use error::{Result, WispZipError};
pub use extract::{extract, Conflict, ExtractOptions, ExtractStats, NameEncoding};
pub use format::{detect, infer_output_format, Classification, Format};
pub use listing::{list_archive, EntryInfo};
pub use progress::{new_cancel_flag, CancelFlag, Phase, Progress};
