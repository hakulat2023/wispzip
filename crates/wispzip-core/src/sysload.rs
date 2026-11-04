//! System load probing: the basis for automatically choosing extract/compress thread counts.
//!
//! On Unix, "1-minute load average / logical cores" acts as a CPU-utilization proxy (>1.0 means overloaded);
//! Windows has no lightweight equivalent, so it returns None (decide by core count and file size only).

/// CPU busyness over the last minute (0.0 idle .. >=1.0 saturated); None when it cannot be probed.
pub(crate) fn cpu_busy_ratio() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut avg = [0f64; 3];
        // SAFETY: avg is a valid writable array pointer; getloadavg writes only nelem elements.
        let n = unsafe { libc::getloadavg(avg.as_mut_ptr(), 3) };
        if n < 1 {
            return None;
        }
        let cores = std::thread::available_parallelism().ok()?.get() as f64;
        Some((avg[0] / cores).clamp(0.0, 4.0))
    }
    #[cfg(windows)]
    {
        None
    }
}

/// Automatically choose the extract worker count from entry count, total bytes and system load.
/// 1 keeps the single-threaded path (spinning up threads is not worth it for small archives).
pub(crate) fn auto_workers(entry_count: usize, total_bytes: u64) -> usize {
    const MIN_ENTRIES: usize = 16;
    const MIN_BYTES: u64 = 1024 * 1024;
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(8);

    if entry_count < MIN_ENTRIES || total_bytes < MIN_BYTES || cores < 2 {
        return 1;
    }

    let workers = match cpu_busy_ratio() {
        // The system is already near saturation: no extra concurrency
        Some(b) if b >= 0.85 => 1,
        // Moderate load: halve concurrency
        Some(b) if b >= 0.60 => (cores / 2).max(2),
        _ => cores,
    };
    workers.min(entry_count).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_archives_stay_single_threaded() {
        assert_eq!(auto_workers(0, 0), 1);
        assert_eq!(auto_workers(4, 1024), 1);
        assert_eq!(auto_workers(8, 4 * 1024 * 1024), 1);
        assert_eq!(auto_workers(64, 512 * 1024), 1);
    }

    #[test]
    fn large_archives_get_workers_capped() {
        let w = auto_workers(1000, 512 * 1024 * 1024);
        assert!((1..=8).contains(&w), "workers out of range: {w}");
    }
}
