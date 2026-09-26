//! Minimal UTC timestamp formatting shared across the crate.
//!
//! Several places (lockfile `generated_at`, snapshot `created_at`/IDs) need
//! a real wall-clock RFC 3339 UTC timestamp. Rather than pull in a full
//! datetime crate, this converts `SystemTime` to civil UTC components using
//! Howard Hinnant's well-known `civil_from_days` algorithm (correct
//! proleptic Gregorian conversion for any date representable by the type).

use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current UTC time as an RFC 3339 timestamp, e.g.
/// `2026-06-29T12:34:56Z`.
pub fn rfc3339_now() -> String {
    format_rfc3339(SystemTime::now())
}

/// Returns the current UTC time formatted for use as a path-safe ID
/// component (`YYYYMMDDTHHMMSSZ`, no colons).
pub fn compact_now() -> String {
    format_compact_utc(SystemTime::now())
}

/// Current Unix epoch seconds (UTC).
pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse a compact RFC 3339 UTC timestamp produced by [`format_rfc3339`]
/// (`YYYY-MM-DDTHH:MM:SSZ`, optional fractional seconds) into Unix seconds.
pub fn parse_rfc3339_unix_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s.strip_suffix('Z').unwrap_or(s);
    let s = s.split('+').next().unwrap_or(s);
    let s = if let Some((main, _)) = s.split_once('.') {
        main
    } else {
        s
    };
    let (date, time) = s.split_once('T')?;
    let mut date_parts = date.split('-');
    let y: i64 = date_parts.next()?.parse().ok()?;
    let m: u32 = date_parts.next()?.parse().ok()?;
    let d: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let h: u32 = time_parts.next()?.parse().ok()?;
    let mi: u32 = time_parts.next()?.parse().ok()?;
    let sec: u32 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    if h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let total = days
        .checked_mul(86_400)?
        .checked_add(i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(sec))?;
    u64::try_from(total).ok()
}

/// Inverse of the civil_from_days algorithm used above (Hinnant).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 {
        (m - 3) as u64
    } else {
        (m + 9) as u64
    };
    let doy = (153 * mp + 2) / 5 + u64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

pub fn format_rfc3339(time: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = civil_utc_from_system_time(time);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

pub fn format_compact_utc(time: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = civil_utc_from_system_time(time);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

fn civil_utc_from_system_time(time: SystemTime) -> (i64, u32, u32, u32, u32, u32) {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = duration.as_secs() as i64;
    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);

    let hour = (secs_of_day / 3600) as u32;
    let minute = ((secs_of_day % 3600) / 60) as u32;
    let second = (secs_of_day % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };

    (year, month, day, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn matches_known_epoch_values() {
        assert_eq!(format_rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        // 2024-03-15T13:45:30Z
        assert_eq!(
            format_rfc3339(UNIX_EPOCH + Duration::from_secs(1_710_510_330)),
            "2024-03-15T13:45:30Z"
        );
        // Leap day: 2000-02-29T00:00:00Z
        assert_eq!(
            format_rfc3339(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
    }

    #[test]
    fn compact_format_has_no_separators() {
        assert_eq!(
            format_compact_utc(UNIX_EPOCH + Duration::from_secs(1_710_510_330)),
            "20240315T134530Z"
        );
    }

    #[test]
    fn now_is_not_a_hardcoded_date() {
        assert_ne!(rfc3339_now(), "2026-06-29T00:00:00Z");
        assert_ne!(compact_now(), "20260629T000000Z");
    }

    #[test]
    fn parse_rfc3339_roundtrips_known_values() {
        assert_eq!(parse_rfc3339_unix_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_unix_secs("2024-03-15T13:45:30Z"),
            Some(1_710_510_330)
        );
        assert_eq!(
            parse_rfc3339_unix_secs("2000-02-29T00:00:00Z"),
            Some(951_782_400)
        );
        assert_eq!(
            parse_rfc3339_unix_secs("2024-03-15T13:45:30.123Z"),
            Some(1_710_510_330)
        );
    }
}
