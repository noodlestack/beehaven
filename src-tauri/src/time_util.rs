use std::time::{SystemTime, UNIX_EPOCH};

/// Pure standard-library UTC timestamp helpers (Howard Hinnant's civil-from-days algorithm).
/// Replaces `chrono` (flagged unmaintained in `RUSTSEC-2026-0543`) with zero dependencies.
fn utc_parts(secs_since_epoch: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs_since_epoch / 86_400) as i64;
    let rem = secs_since_epoch % 86_400;
    let hour = (rem / 3_600) as u32;
    let min = ((rem % 3_600) / 60) as u32;
    let sec = (rem % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (if m <= 2 { y + 1 } else { y }) as i32;

    (year, m, d, hour, min, sec)
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Formats current UTC time as `YYYY-MM-DDTHH:MM:SSZ` (RFC 3339).
pub fn now_rfc3339() -> String {
    format_rfc3339(current_epoch_secs())
}

/// Formats current UTC time as `YYYYMMDDTHHMMSSZ` (safe for filenames).
pub fn now_compact_utc() -> String {
    let (y, m, d, hh, mm, ss) = utc_parts(current_epoch_secs());
    format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z")
}

pub fn format_rfc3339(secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = utc_parts(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::format_rfc3339;

    #[test]
    fn formats_known_utc_epochs_accurately() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_795_785_600), "2026-11-27T13:20:00Z");
    }
}
