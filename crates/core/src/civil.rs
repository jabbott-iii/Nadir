//! UTC calendar time ("civil time") from system time, for log lines and
//! file names.
//!
//! There are no time zones and no leap seconds here: like Unix time, every
//! day has exactly 86 400 seconds.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: i64 = 86_400;

/// A UTC date and time in the proleptic Gregorian calendar.
///
/// Values compare in chronological order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    /// The year, for example `2026`.
    pub year: i64,
    /// The month, `1..=12`.
    pub month: u8,
    /// The day of the month, `1..=31`.
    pub day: u8,
    /// The hour, `0..=23`.
    pub hour: u8,
    /// The minute, `0..=59`.
    pub minute: u8,
    /// The second, `0..=59`.
    pub second: u8,
    /// The millisecond, `0..=999`.
    pub millisecond: u16,
}

impl DateTime {
    /// Returns the current UTC date and time.
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    /// Converts a [`SystemTime`] to a UTC date and time, truncating to the
    /// millisecond. Times before 1970 are handled.
    pub fn from_system_time(time: SystemTime) -> Self {
        let (seconds, nanos) = match time.duration_since(UNIX_EPOCH) {
            Ok(after) => (saturating_i64(after.as_secs()), after.subsec_nanos()),
            Err(error) => {
                // `before` counts backwards from the epoch. Round down to
                // the second below and count the remainder forwards.
                let before = error.duration();
                match before.subsec_nanos() {
                    0 => (-saturating_i64(before.as_secs()), 0),
                    nanos => (-saturating_i64(before.as_secs()) - 1, 1_000_000_000 - nanos),
                }
            }
        };
        Self::from_unix(seconds, (nanos / 1_000_000) as u16)
    }

    /// Converts whole seconds since 1970-01-01T00:00:00Z (Unix time) to a
    /// UTC date and time. Negative values are before 1970.
    pub fn from_unix_seconds(seconds: i64) -> Self {
        Self::from_unix(seconds, 0)
    }

    fn from_unix(seconds: i64, millisecond: u16) -> Self {
        let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
        let second_of_day = seconds.rem_euclid(SECONDS_PER_DAY);
        Self {
            year,
            month,
            day,
            hour: (second_of_day / 3_600) as u8,
            minute: (second_of_day / 60 % 60) as u8,
            second: (second_of_day % 60) as u8,
            millisecond,
        }
    }

    /// Formats the date and time for use in a file name, to the second:
    /// `2026-10-12_19-28-03`.
    ///
    /// The result has no characters that any supported OS forbids in file
    /// names, and sorts chronologically.
    pub fn file_stamp(self) -> String {
        let Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            ..
        } = self;
        format!("{year:04}-{month:02}-{day:02}_{hour:02}-{minute:02}-{second:02}")
    }
}

/// Formats as RFC 3339 with milliseconds: `2026-10-12T19:28:03.123Z`.
impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
        } = *self;
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z"
        )
    }
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Converts a count of days since 1970-01-01 to `(year, month, day)`.
///
/// This is the well-known days-to-civil algorithm. It works in 400-year
/// eras that start on 1 March, so that the leap day is the last day of a
/// year and month lengths follow a simple pattern.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    const DAYS_PER_ERA: i64 = 146_097;
    /// Days from 0000-03-01 to 1970-01-01.
    const EPOCH_SHIFT: i64 = 719_468;

    let shifted = days + EPOCH_SHIFT;
    let era = shifted.div_euclid(DAYS_PER_ERA);
    let day_of_era = shifted.rem_euclid(DAYS_PER_ERA);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    // Day and month within a year that starts on 1 March.
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    // January and February belong to the next calendar year.
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn date_time(year: i64, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        }
    }

    #[test]
    fn matches_known_timestamps() {
        // Expected values are from Python's `datetime`.
        let cases = [
            (0, date_time(1970, 1, 1, 0, 0, 0)),
            (-1, date_time(1969, 12, 31, 23, 59, 59)),
            (86_399, date_time(1970, 1, 1, 23, 59, 59)),
            (86_400, date_time(1970, 1, 2, 0, 0, 0)),
            (1_000_000_000, date_time(2001, 9, 9, 1, 46, 40)),
            (1_234_567_890, date_time(2009, 2, 13, 23, 31, 30)),
            (-2_208_988_800, date_time(1900, 1, 1, 0, 0, 0)),
            (-11_644_473_600, date_time(1601, 1, 1, 0, 0, 0)),
            (32_503_680_000, date_time(3000, 1, 1, 0, 0, 0)),
            (253_402_300_799, date_time(9999, 12, 31, 23, 59, 59)),
        ];
        for (seconds, expected) in cases {
            assert_eq!(DateTime::from_unix_seconds(seconds), expected, "{seconds}");
        }
    }

    #[test]
    fn handles_leap_years() {
        let cases = [
            // 2000 is divisible by 400: a leap year.
            (951_782_399, date_time(2000, 2, 28, 23, 59, 59)),
            (951_782_400, date_time(2000, 2, 29, 0, 0, 0)),
            (951_868_800, date_time(2000, 3, 1, 0, 0, 0)),
            // 2024 is divisible by 4: a leap year.
            (1_709_164_800, date_time(2024, 2, 29, 0, 0, 0)),
            (1_709_251_199, date_time(2024, 2, 29, 23, 59, 59)),
            (1_709_251_200, date_time(2024, 3, 1, 0, 0, 0)),
            // 2100 is divisible by 100 but not 400: not a leap year.
            (4_107_542_399, date_time(2100, 2, 28, 23, 59, 59)),
            (4_107_542_400, date_time(2100, 3, 1, 0, 0, 0)),
        ];
        for (seconds, expected) in cases {
            assert_eq!(DateTime::from_unix_seconds(seconds), expected, "{seconds}");
        }
    }

    #[test]
    fn crosses_the_2038_boundary() {
        let last_i32 = i64::from(i32::MAX);
        assert_eq!(
            DateTime::from_unix_seconds(last_i32),
            date_time(2038, 1, 19, 3, 14, 7)
        );
        assert_eq!(
            DateTime::from_unix_seconds(last_i32 + 1),
            date_time(2038, 1, 19, 3, 14, 8)
        );
    }

    /// The obvious calendar rules, written with no cleverness.
    fn days_in_month(year: i64, month: u8) -> u8 {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        match month {
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 31,
        }
    }

    #[test]
    fn matches_a_naive_calendar_for_every_day_of_800_years() {
        // Forwards from 1970-01-01 for one full 400-year cycle.
        let (mut year, mut month, mut day) = (1970, 1, 1);
        for days in 0..=146_097 {
            assert_eq!(civil_from_days(days), (year, month, day), "day {days}");
            day += 1;
            if day > days_in_month(year, month) {
                day = 1;
                month += 1;
                if month > 12 {
                    month = 1;
                    year += 1;
                }
            }
        }
        // Backwards for another.
        let (mut year, mut month, mut day) = (1970, 1, 1);
        for days in (-146_097..=0).rev() {
            assert_eq!(civil_from_days(days), (year, month, day), "day {days}");
            day -= 1;
            if day == 0 {
                month -= 1;
                if month == 0 {
                    month = 12;
                    year -= 1;
                }
                day = days_in_month(year, month);
            }
        }
    }

    #[test]
    fn extreme_inputs_do_not_overflow() {
        for seconds in [i64::MIN, i64::MIN + 1, -1, i64::MAX - 1, i64::MAX] {
            let dt = DateTime::from_unix_seconds(seconds);
            assert!((1..=12).contains(&dt.month), "{seconds}: {dt:?}");
            assert!((1..=31).contains(&dt.day), "{seconds}: {dt:?}");
            assert!(
                dt.hour < 24 && dt.minute < 60 && dt.second < 60,
                "{seconds}: {dt:?}"
            );
        }
        assert!(DateTime::from_unix_seconds(i64::MAX).year > 292_000_000_000);
        assert!(DateTime::from_unix_seconds(i64::MIN).year < -292_000_000_000);
    }

    #[test]
    fn converts_system_time_with_milliseconds() {
        let time = UNIX_EPOCH + Duration::new(1_234_567_890, 123_999_999);
        let expected = DateTime {
            millisecond: 123,
            ..date_time(2009, 2, 13, 23, 31, 30)
        };
        assert_eq!(DateTime::from_system_time(time), expected);
        assert_eq!(
            DateTime::from_system_time(UNIX_EPOCH),
            date_time(1970, 1, 1, 0, 0, 0)
        );
    }

    #[test]
    fn converts_system_time_before_the_epoch() {
        let whole = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(
            DateTime::from_system_time(whole),
            date_time(1969, 12, 31, 23, 59, 59)
        );

        // A quarter of a second before the epoch is 23:59:59.750.
        let fractional = UNIX_EPOCH - Duration::from_millis(250);
        let expected = DateTime {
            millisecond: 750,
            ..date_time(1969, 12, 31, 23, 59, 59)
        };
        assert_eq!(DateTime::from_system_time(fractional), expected);
    }

    #[test]
    fn now_is_plausible() {
        let now = DateTime::now();
        assert!(now.year >= 2026, "{now}");
    }

    #[test]
    fn formats_for_logs_and_file_names() {
        let dt = DateTime {
            millisecond: 7,
            ..date_time(2026, 10, 2, 9, 8, 3)
        };
        assert_eq!(dt.to_string(), "2026-10-02T09:08:03.007Z");
        assert_eq!(dt.file_stamp(), "2026-10-02_09-08-03");
        assert_eq!(
            date_time(987, 1, 1, 0, 0, 0).to_string(),
            "0987-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn orders_chronologically() {
        let mut previous = DateTime::from_unix_seconds(-100_000_000);
        for seconds in (-100_000_000 + 9_999_991..2_000_000_000).step_by(9_999_991) {
            let current = DateTime::from_unix_seconds(seconds);
            assert!(previous < current, "{previous} is not before {current}");
            previous = current;
        }
    }
}
