use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::error::cancel_io_error;

/// Process phases, reported through [`Progress::phase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepare,
    Compress,
    Extract,
    Finish,
}

/// Snapshot of job progress. `processed_bytes` counts uncompressed input for
/// zip compression and uncompressed output for extraction; for tar.gz-style
/// pipelines it counts bytes consumed from the (compressed) archive stream.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub phase: Phase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
    pub processed_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    pub output_bytes: u64,
    pub files_done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_total: Option<u64>,
}

/// Shared cancellation flag: set by the UI (`cancel_job`) or when the progress
/// callback returns `false`.
pub type CancelFlag = Arc<AtomicBool>;

pub fn new_cancel_flag() -> CancelFlag {
    Arc::new(AtomicBool::new(false))
}

const EMIT_INTERVAL: Duration = Duration::from_millis(80);
const EMIT_MIN_BYTES: u64 = 1 << 20;

/// Accumulates progress counters and throttles callback invocations. Shared
/// between the IO-counting wrappers and the entry loops through `Rc<RefCell>`.
pub struct Reporter<F> {
    pub phase: Phase,
    pub current: Option<String>,
    pub processed: u64,
    pub output: u64,
    pub files_done: u64,
    pub files_total: Option<u64>,
    pub total: Option<u64>,
    pub cancel: CancelFlag,
    cb: F,
    last_emit: Instant,
    last_emit_bytes: u64,
}

pub type Shared<F> = Rc<RefCell<Reporter<F>>>;

impl<F: FnMut(&Progress) -> bool> Reporter<F> {
    pub fn new(cancel: CancelFlag, cb: F) -> Shared<F> {
        Rc::new(RefCell::new(Reporter {
            phase: Phase::Prepare,
            current: None,
            processed: 0,
            output: 0,
            files_done: 0,
            files_total: None,
            total: None,
            cancel,
            cb,
            last_emit: Instant::now() - EMIT_INTERVAL,
            last_emit_bytes: 0,
        }))
    }

    /// Invoke the callback if throttling allows it. Returning `false` from the
    /// callback flips the cancel flag.
    pub fn report(&mut self, force: bool) {
        if !force
            && self.last_emit.elapsed() < EMIT_INTERVAL
            && self.processed.saturating_sub(self.last_emit_bytes) < EMIT_MIN_BYTES
        {
            return;
        }
        self.last_emit = Instant::now();
        self.last_emit_bytes = self.processed;
        let p = Progress {
            phase: self.phase,
            current_file: self.current.clone(),
            processed_bytes: self.processed,
            total_bytes: self.total,
            output_bytes: self.output,
            files_done: self.files_done,
            files_total: self.files_total,
        };
        if !(self.cb)(&p) {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub fn progress(&self) -> Progress {
        Progress {
            phase: self.phase,
            current_file: self.current.clone(),
            processed_bytes: self.processed,
            total_bytes: self.total,
            output_bytes: self.output,
            files_done: self.files_done,
            files_total: self.files_total,
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Cancellation check for engine loops (core error type).
    pub fn check_cancel(&self) -> crate::error::Result<()> {
        if self.cancelled() {
            Err(crate::error::WispZipError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Cancellation check for IO wrappers (io error type, marker-tagged).
    pub fn check_cancel_io(&self) -> io::Result<()> {
        if self.cancelled() {
            Err(cancel_io_error())
        } else {
            Ok(())
        }
    }
}

/// Reader that counts every byte pulled through it, checks cancellation and
/// drives the progress callback.
pub struct CountingReader<R, F> {
    inner: R,
    shared: Shared<F>,
}

impl<R: io::Read, F: FnMut(&Progress) -> bool> CountingReader<R, F> {
    pub fn new(inner: R, shared: Shared<F>) -> Self {
        Self { inner, shared }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: io::Read, F: FnMut(&Progress) -> bool> io::Read for CountingReader<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.shared.borrow().check_cancel_io()?;
        let n = self.inner.read(buf)?;
        if n > 0 {
            let mut r = self.shared.borrow_mut();
            r.processed += n as u64;
            r.report(false);
        }
        Ok(n)
    }
}

pub struct CountingWriter<W, F> {
    inner: W,
    shared: Shared<F>,
}

impl<W: io::Write, F: FnMut(&Progress) -> bool> CountingWriter<W, F> {
    pub fn new(inner: W, shared: Shared<F>) -> Self {
        Self { inner, shared }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

/// `ZipWriter` requires `Seek` on its sink; forward it untouched so counting
/// stays byte-accurate (seeks only reposition, they never write).
impl<W: io::Seek, F> io::Seek for CountingWriter<W, F> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl<W: io::Write, F: FnMut(&Progress) -> bool> io::Write for CountingWriter<W, F> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.shared.borrow().check_cancel_io()?;
        let n = self.inner.write(buf)?;
        if n > 0 {
            let mut r = self.shared.borrow_mut();
            r.output += n as u64;
            r.report(false);
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn system_time_from_unix(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reporter_forwards_cancel_from_callback() {
        let shared = Reporter::new(new_cancel_flag(), |_| false);
        shared.borrow_mut().report(true);
        assert!(shared.borrow().cancelled());
    }

    #[test]
    fn reporter_stays_running_when_callback_true() {
        let shared = Reporter::new(new_cancel_flag(), |_| true);
        shared.borrow_mut().report(true);
        assert!(!shared.borrow().cancelled());
    }

    #[test]
    fn check_cancel_flags_both_error_types() {
        let shared: Shared<_> = Reporter::new(new_cancel_flag(), |_| true);
        assert!(shared.borrow().check_cancel().is_ok());
        assert!(shared.borrow().check_cancel_io().is_ok());
        shared.borrow().cancel.store(true, Ordering::SeqCst);
        assert!(shared.borrow().check_cancel().is_err());
        assert!(shared.borrow().check_cancel_io().is_err());
    }

    #[test]
    fn unix_secs_roundtrip() {
        let t = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        assert_eq!(unix_secs(t), 1_767_225_600);
        assert_eq!(unix_secs(UNIX_EPOCH), 0);
        assert_eq!(system_time_from_unix(1_767_225_600), t);
    }

    #[test]
    fn counting_writer_counts_and_reports() {
        let shared = Reporter::new(new_cancel_flag(), |_| true);
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut cw = CountingWriter::new(&mut buf, shared.clone());
            cw.write_all(b"hello").unwrap();
            cw.write_all(b" world").unwrap();
            cw.flush().unwrap();
        }
        assert_eq!(buf.len(), 11);
        assert_eq!(shared.borrow().output, 11);
    }

    #[test]
    fn counting_reader_counts_and_reports() {
        use std::io::Read;
        let shared = Reporter::new(new_cancel_flag(), |_| true);
        let mut r = CountingReader::new(&b"abcdef"[..], shared.clone());
        let mut s = String::new();
        r.read_to_string(&mut s).unwrap();
        assert_eq!(s, "abcdef");
        assert_eq!(shared.borrow().processed, 6);
    }

    #[test]
    fn write_through_cancelled_reporter_errors() {
        let shared = Reporter::new(new_cancel_flag(), |_| false);
        shared.borrow().cancel.store(true, Ordering::SeqCst);
        let mut buf: Vec<u8> = Vec::new();
        let mut cw = CountingWriter::new(&mut buf, shared);
        let err = cw.write_all(b"x");
        assert!(err.is_err(), "write must fail once cancelled");
    }
}
