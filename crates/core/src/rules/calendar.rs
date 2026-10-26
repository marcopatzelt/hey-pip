//! Business days: Mon–Fri, not a public holiday in NRW. OWNER: agent "memory-rules".
//!
//! Holidays are computed from the Easter date (Gregorian computus) so the calendar keeps
//! working after 2027; the tests pin every NRW holiday of 2026 and 2027 by hand.

use chrono::{Datelike, Days, NaiveDate, Weekday};

pub struct Calendar;

impl Calendar {
    /// NRW public holidays: Neujahr, Karfreitag, Ostermontag, 1. Mai, Christi Himmelfahrt,
    /// Pfingstmontag, Fronleichnam, Tag der Deutschen Einheit, Allerheiligen, 25./26. Dezember.
    pub fn is_holiday_nrw(d: NaiveDate) -> bool {
        let fixed = matches!((d.month(), d.day()), (1, 1) | (5, 1) | (10, 3) | (11, 1) | (12, 25) | (12, 26));
        if fixed {
            return true;
        }
        let Some(easter) = easter_sunday(d.year()) else { return false };
        let offset = d.signed_duration_since(easter).num_days();
        // Karfreitag, Ostermontag, Himmelfahrt, Pfingstmontag, Fronleichnam
        matches!(offset, -2 | 1 | 39 | 50 | 60)
    }

    /// Mon–Fri and not an NRW holiday. Saturday is NOT a business day (§ 46 Satz 2 SGB V,
    /// § 64 Abs. 3 SGG).
    pub fn is_business_day(d: NaiveDate) -> bool {
        !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) && !Self::is_holiday_nrw(d)
    }

    /// First business day strictly after `d`.
    pub fn next_business_day(d: NaiveDate) -> NaiveDate {
        let mut n = d;
        loop {
            n = n.succ_opt().expect("date in range");
            if Self::is_business_day(n) {
                return n;
            }
        }
    }

    /// `d` itself if it is a business day, else the next one (end of a deadline, § 64 Abs. 3 SGG).
    pub fn this_or_next_business_day(d: NaiveDate) -> NaiveDate {
        if Self::is_business_day(d) { d } else { Self::next_business_day(d) }
    }
}

/// Easter Sunday (anonymous Gregorian algorithm, Meeus/Jones/Butcher).
fn easter_sunday(year: i32) -> Option<NaiveDate> {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32)
}

/// `d` minus `n` days (saturating at the chrono minimum, never panics in practice).
pub(crate) fn days_before(d: NaiveDate, n: u64) -> NaiveDate {
    d.checked_sub_days(Days::new(n)).unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    const NRW_2026: [(u32, u32); 11] =
        [(1, 1), (4, 3), (4, 6), (5, 1), (5, 14), (5, 25), (6, 4), (10, 3), (11, 1), (12, 25), (12, 26)];
    const NRW_2027: [(u32, u32); 11] =
        [(1, 1), (3, 26), (3, 29), (5, 1), (5, 6), (5, 17), (5, 27), (10, 3), (11, 1), (12, 25), (12, 26)];

    #[test]
    fn easter_dates() {
        assert_eq!(easter_sunday(2026), Some(d(2026, 4, 5)));
        assert_eq!(easter_sunday(2027), Some(d(2027, 3, 28)));
        assert_eq!(easter_sunday(2024), Some(d(2024, 3, 31)));
        assert_eq!(easter_sunday(2025), Some(d(2025, 4, 20)));
    }

    #[test]
    fn nrw_holidays_2026_2027_exactly() {
        for (year, list) in [(2026, NRW_2026), (2027, NRW_2027)] {
            let mut day = d(year, 1, 1);
            let mut found = Vec::new();
            while day.year() == year {
                if Calendar::is_holiday_nrw(day) {
                    found.push((day.month(), day.day()));
                }
                day = day.succ_opt().unwrap();
            }
            assert_eq!(found, list.to_vec(), "holidays {year}");
        }
    }

    #[test]
    fn not_holidays_in_nrw() {
        // Heiligabend, Silvester, Rosenmontag, Reformationstag, Buß- und Bettag, Ostersonntag is a Sunday anyway
        for x in [d(2026, 12, 24), d(2026, 12, 31), d(2026, 2, 16), d(2026, 10, 31), d(2026, 11, 18)] {
            assert!(!Calendar::is_holiday_nrw(x), "{x}");
        }
    }

    #[test]
    fn business_days() {
        assert!(Calendar::is_business_day(d(2026, 10, 30))); // Friday
        assert!(!Calendar::is_business_day(d(2026, 10, 31))); // Saturday
        assert!(!Calendar::is_business_day(d(2026, 11, 1))); // Sunday + Allerheiligen
        assert!(Calendar::is_business_day(d(2026, 11, 2))); // Monday
        assert!(!Calendar::is_business_day(d(2026, 10, 3))); // Saturday + Einheit
        assert!(Calendar::is_business_day(d(2027, 10, 4))); // Monday after Einheit 2027 (Sunday): no substitute day
    }

    #[test]
    fn next_business_day_cases() {
        assert_eq!(Calendar::next_business_day(d(2026, 10, 30)), d(2026, 11, 2)); // Fri -> Mon
        assert_eq!(Calendar::next_business_day(d(2026, 10, 29)), d(2026, 10, 30)); // Thu -> Fri
        assert_eq!(Calendar::next_business_day(d(2026, 10, 31)), d(2026, 11, 2)); // Sat -> Mon
        assert_eq!(Calendar::next_business_day(d(2026, 4, 2)), d(2026, 4, 7)); // Thu before Easter -> Tue after
        assert_eq!(Calendar::next_business_day(d(2026, 5, 13)), d(2026, 5, 15)); // before Himmelfahrt
        assert_eq!(Calendar::next_business_day(d(2026, 12, 24)), d(2026, 12, 28)); // Christmas Fri/Sat
        assert_eq!(Calendar::next_business_day(d(2026, 12, 31)), d(2027, 1, 4)); // New Year Fri
        assert_eq!(Calendar::next_business_day(d(2027, 3, 25)), d(2027, 3, 30)); // Easter 2027
        assert_eq!(Calendar::next_business_day(d(2027, 5, 26)), d(2027, 5, 28)); // Fronleichnam 2027
    }

    #[test]
    fn this_or_next() {
        assert_eq!(Calendar::this_or_next_business_day(d(2026, 11, 2)), d(2026, 11, 2));
        assert_eq!(Calendar::this_or_next_business_day(d(2026, 10, 31)), d(2026, 11, 2));
    }
}
