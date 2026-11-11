//! The wire form of a [`Timestamp`]: `2026-11-06T10:11:12.123456Z`, UTC with microseconds, always `Z`.

use sc_clock::Timestamp;

pub(crate) fn rfc3339(at: Timestamp) -> String {
    let micros = at.as_unix_micros();
    let secs = micros.div_euclid(1_000_000);
    let sub = micros.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let in_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{sub:06}Z",
        in_day / 3600,
        in_day % 3600 / 60,
        in_day % 60
    )
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(micros: i64) -> String {
        rfc3339(Timestamp::from_unix_micros(micros))
    }

    #[test]
    fn epoch_leap_day_and_before_epoch_are_written_in_utc_with_microseconds() {
        // Arrange
        let instants = [
            0,
            951_782_400_000_000,
            1_793_956_272_123_456,
            -1,
            -62_135_596_800_000_000,
        ];

        // Act
        let written: Vec<String> = instants.into_iter().map(at).collect();

        // Assert
        assert_eq!(
            written,
            [
                "1970-01-01T00:00:00.000000Z",
                "2000-02-29T00:00:00.000000Z",
                "2026-11-06T09:11:12.123456Z",
                "1969-12-31T23:59:59.999999Z",
                "0001-01-01T00:00:00.000000Z",
            ]
        );
    }
}
