//! Backup slot and object naming, mirroring `github.com/suhlig/sqlite-vault`
//! so that object names stay interchangeable with that tooling.
//!
//! Retention works by overwriting: each slot has a deterministic name, so a new
//! backup for a slot replaces the previous one (bucket versioning must be
//! disabled). This keeps at most 24 hourly + 7 daily + 53 weekly + yearly
//! objects in the bucket, with no deletion step.

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Hourly,
    Daily,
    Weekly,
    Yearly,
}

impl Slot {
    pub fn label(self) -> &'static str {
        match self {
            Slot::Hourly => "hourly",
            Slot::Daily => "daily",
            Slot::Weekly => "weekly",
            Slot::Yearly => "yearly",
        }
    }
}

/// The backup slot for the given time.
///
/// At 04:00 on days other than Sunday a daily backup is made instead of the
/// hourly one; at 04:00 on Sundays a weekly backup instead of the daily; and at
/// 04:00 on the last Sunday of the year a yearly backup instead of the weekly.
pub fn slot_of(now: DateTime<Utc>) -> Slot {
    if now.hour() == 4 {
        if now.weekday() == Weekday::Sun {
            if last_sunday_of_year(now) {
                Slot::Yearly
            } else {
                Slot::Weekly
            }
        } else {
            Slot::Daily
        }
    } else {
        Slot::Hourly
    }
}

/// Object name for a backup taken at `now`, e.g. `kehrkraft.hourly-09.db.age`.
pub fn object_name(prefix: &str, now: DateTime<Utc>) -> String {
    format!("{prefix}.{}.db.age", slot_object_part(now))
}

/// Alias object name for the most recent backup of the given slot, e.g.
/// `kehrkraft.hourly-latest.alias`. Its content is the object name of the last
/// successful backup for that slot.
pub fn latest_alias_name(prefix: &str, slot: Slot) -> String {
    format!("{prefix}.{}-latest.alias", slot.label())
}

fn slot_object_part(now: DateTime<Utc>) -> String {
    match slot_of(now) {
        Slot::Yearly => format!("yearly-{:04}", now.year()),
        Slot::Weekly => format!("weekly-{:02}", now.iso_week().week()),
        Slot::Daily => format!("daily-{}", now.format("%A")),
        Slot::Hourly => format!("hourly-{:02}", now.hour()),
    }
}

/// Whether `now` falls on the last Sunday on or before December 31 of its year.
fn last_sunday_of_year(now: DateTime<Utc>) -> bool {
    let dec31 = NaiveDate::from_ymd_opt(now.year(), 12, 31)
        .expect("December 31 is always a valid date")
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always valid")
        .and_utc();
    // weekday().num_days_from_sunday() is 0 for Sunday .. 6 for Saturday.
    let offset = dec31.weekday().num_days_from_sunday() as i64;
    let last_sunday = dec31 - chrono::Duration::days(offset);
    now.date_naive() == last_sunday.date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn hourly_slot_outside_four_am() {
        for hour in [0u32, 3, 5, 10, 23] {
            let t = utc(&format!("2026-09-07T{hour:02}:00:00Z"));
            assert_eq!(slot_of(t), Slot::Hourly);
            assert_eq!(
                object_name("kehrkraft", t),
                format!("kehrkraft.hourly-{hour:02}.db.age")
            );
        }
    }

    #[test]
    fn four_am_on_a_weekday_is_daily() {
        // 2026-09-07 is a Monday.
        let t = utc("2026-09-07T04:00:00Z");
        assert_eq!(slot_of(t), Slot::Daily);
        assert_eq!(object_name("kehrkraft", t), "kehrkraft.daily-Monday.db.age");

        // 2026-09-08 is a Tuesday.
        let t = utc("2026-09-08T04:00:00Z");
        assert_eq!(slot_of(t), Slot::Daily);
        assert_eq!(
            object_name("kehrkraft", t),
            "kehrkraft.daily-Tuesday.db.age"
        );
    }

    #[test]
    fn four_am_on_a_sunday_is_weekly() {
        // 2026-09-06 is a Sunday, ISO week 36 of 2026.
        let t = utc("2026-09-06T04:00:00Z");
        assert_eq!(slot_of(t), Slot::Weekly);
        assert_eq!(object_name("kehrkraft", t), "kehrkraft.weekly-36.db.age");
    }

    #[test]
    fn sunday_of_iso_week_53_is_still_weekly() {
        // 2027-01-03 (Sunday) belongs to ISO week 53 of 2026.
        let t = utc("2027-01-03T04:00:00Z");
        assert_eq!(slot_of(t), Slot::Weekly);
        assert_eq!(object_name("kehrkraft", t), "kehrkraft.weekly-53.db.age");
    }

    #[test]
    fn last_sunday_of_year_is_yearly() {
        // 2026-12-27 is the last Sunday of 2026 (Dec 31 is a Thursday).
        let t = utc("2026-12-27T04:00:00Z");
        assert_eq!(slot_of(t), Slot::Yearly);
        assert_eq!(object_name("kehrkraft", t), "kehrkraft.yearly-2026.db.age");

        // Only the 04:00 hour of that day is "yearly"; the rest is hourly.
        let t = utc("2026-12-27T23:00:00Z");
        assert_eq!(slot_of(t), Slot::Hourly);
        assert_eq!(object_name("kehrkraft", t), "kehrkraft.hourly-23.db.age");
    }

    #[test]
    fn alias_names_per_slot() {
        assert_eq!(
            latest_alias_name("kehrkraft", Slot::Hourly),
            "kehrkraft.hourly-latest.alias"
        );
        assert_eq!(
            latest_alias_name("kehrkraft", Slot::Daily),
            "kehrkraft.daily-latest.alias"
        );
        assert_eq!(
            latest_alias_name("kehrkraft", Slot::Weekly),
            "kehrkraft.weekly-latest.alias"
        );
        assert_eq!(
            latest_alias_name("kehrkraft", Slot::Yearly),
            "kehrkraft.yearly-latest.alias"
        );
    }
}
