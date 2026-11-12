//! One module per archive/stream format. Each module exposes the same three
//! entry points so the hosts (compress / extract / listing) dispatch uniformly:
//!
//! - `write(entries, …)`  — compress side (where the format supports it)
//! - `extract(archive, …)` — decompress side
//! - `list(archive, …)`   — read-only entry preview
//!
//! Adding a format: create a module here, add a `Format` variant in
//! `format.rs`, and extend the three dispatch sites in the hosts.

pub mod rar;
pub mod sevenz;
pub mod streams;
pub mod tar;
pub mod zip;

pub(crate) use crate::listing::EntryInfo;

/// Howard Hinnant's civil_from_days: days since 1970-01-01 -> (y, m, d).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Inverse of [civil_from_days]: (y, m, d) -> days since 1970-01-01.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for &(y, m, d) in &[
            (1970, 1, 1),
            (1980, 1, 1),
            (2000, 2, 29),
            (2026, 8, 28),
            (2107, 12, 31),
        ] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
    }
}
