//! Public iCalendar feed for a building's Kehrwoche duties.
//!
//! Served at `/p/{secret_slug}/kehrwoche.ics`: one all-day `VEVENT` per
//! Kehrwoche week (Monday to Sunday) of the current and the following ISO
//! year, so residents can subscribe to the building's cleaning schedule in
//! their calendar app without logging in. The feed never requires Typst.

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
};
use chrono::{Datelike, Duration, Local, Utc};

use crate::db::{queries, Db};
use crate::scheduler::{self, WeekAssignment};
use crate::web::pdf::sanitize_filename;

/// The feed always covers the current ISO year and the following one, so a
/// subscription reaches at least a full year ahead and never goes stale on
/// January 1st.
const FEED_YEARS: i32 = 2;

pub async fn public_ical(
    Path(secret_slug): Path<String>,
    State(pool): State<Db>,
) -> impl IntoResponse {
    let building = match queries::get_building_by_slug(&pool, &secret_slug).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Building not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let year = Local::now().year();
    let mut schedules = Vec::with_capacity(FEED_YEARS as usize);
    for y in year..year + FEED_YEARS {
        match scheduler::schedule_for_year(&building.id, y, &pool).await {
            Ok(s) => schedules.push(s),
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to compute schedule",
                )
                    .into_response()
            }
        }
    }

    let body = ical_document(&building.name, &building.secret_slug, &schedules);
    let filename = format!("Kehrwoche-{}.ics", sanitize_filename(&building.name));
    let cd_val = format!("inline; filename=\"{}\"", filename);
    let cd = HeaderValue::from_str(&cd_val).unwrap_or_else(|_| HeaderValue::from_static("inline"));

    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/calendar; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, cd),
        ],
        body,
    )
        .into_response()
}

/// Compose the full iCalendar document: the calendar header, one all-day
/// event per week of each schedule, and the closing `END:VCALENDAR`.
fn ical_document(building_name: &str, slug: &str, schedules: &[Vec<WeekAssignment>]) -> String {
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\n");
    push_ical_line(&mut out, "VERSION", "2.0");
    push_ical_line(&mut out, "PRODID", "-//Kehrkraft//Kehrwoche//DE");
    push_ical_line(&mut out, "CALSCALE", "GREGORIAN");
    // Widely supported (though non-standard) calendar title that Apple and
    // Google calendars show for the subscription.
    push_ical_line(
        &mut out,
        "X-WR-CALNAME",
        &format!("Kehrwoche {building_name}"),
    );
    for schedule in schedules {
        for week in schedule {
            out.push_str(&event_block(building_name, slug, week));
        }
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

/// One all-day `VEVENT` for a single Kehrwoche week (Monday to Sunday).
/// `DTEND` is exclusive per RFC 5545, so it points to the following Monday.
fn event_block(building_name: &str, slug: &str, w: &WeekAssignment) -> String {
    let name = w.assignee_name.as_deref().unwrap_or("Nicht zugewiesen");
    let summary = format!("Kehrwoche KW {}: {}", w.iso_week, name);
    let description = format!(
        "Gebäude: {building_name}\n{} – {}",
        w.start.format("%d.%m.%Y"),
        w.end.format("%d.%m.%Y")
    );

    let mut out = String::new();
    out.push_str("BEGIN:VEVENT\r\n");
    push_ical_line(&mut out, "UID", &uid_for(slug, w.year, w.iso_week));
    push_ical_line(
        &mut out,
        "DTSTAMP",
        &Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
    );
    push_ical_line(
        &mut out,
        "DTSTART;VALUE=DATE",
        &w.start.format("%Y%m%d").to_string(),
    );
    push_ical_line(
        &mut out,
        "DTEND;VALUE=DATE",
        &(w.end + Duration::days(1)).format("%Y%m%d").to_string(),
    );
    push_ical_line(&mut out, "SUMMARY", &summary);
    push_ical_line(&mut out, "DESCRIPTION", &description);
    out.push_str("END:VEVENT\r\n");
    out
}

/// Stable, unique event identifier across regenerations of the feed, so
/// subscribing calendar apps replace events instead of duplicating them.
/// The secret slug is unique per building and never changes.
fn uid_for(slug: &str, year: i32, iso_week: u32) -> String {
    format!("kehrwoche-{year}-{iso_week:02}-{slug}@kehrkraft")
}

/// Append one escaped and folded `NAME:value` line (CRLF-terminated) to `out`.
fn push_ical_line(out: &mut String, name: &str, value: &str) {
    let line = format!("{name}:{}", escape_ical(value));
    out.push_str(&fold_line(&line));
    out.push_str("\r\n");
}

/// Escape a property value per RFC 5545 §3.3.11: backslash, semicolon, comma,
/// and newline.
fn escape_ical(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Fold a single iCal line into physical lines of at most 75 octets
/// (RFC 5545 §3.1), continuing on new lines that start with a single space.
/// Breaks on character boundaries so multi-byte UTF-8 sequences stay intact.
fn fold_line(line: &str) -> String {
    if line.len() <= 75 {
        return line.to_string();
    }
    let mut out = String::new();
    let mut current = String::new();
    let mut first = true;
    for ch in line.chars() {
        // The first physical line may carry 75 octets; every continuation
        // line additionally carries its leading space, leaving 74 octets.
        let limit = if first { 75 } else { 74 };
        if current.len() + ch.len_utf8() > limit {
            out.push_str(&current);
            out.push_str("\r\n ");
            first = false;
            current.clear();
        }
        current.push(ch);
    }
    out.push_str(&current);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn week(year: i32, iso_week: u32, name: Option<&str>) -> WeekAssignment {
        let start = NaiveDate::from_isoywd_opt(year, iso_week, chrono::Weekday::Mon)
            .expect("valid ISO date");
        WeekAssignment {
            year,
            iso_week,
            start,
            end: start + Duration::days(6),
            assignee_id: name.map(|_| format!("id-{year}-{iso_week}")),
            assignee_name: name.map(str::to_string),
            assignee_email: None,
            delegated: false,
        }
    }

    #[test]
    fn event_covers_monday_to_sunday_with_exclusive_end() {
        let doc = ical_document("Baumhaus", "slug", &[vec![week(2026, 12, Some("Anna"))]]);
        assert!(doc.contains("DTSTART;VALUE=DATE:20260316"), "{doc}");
        // The week ends Sunday 2026-03-22; DTEND is exclusive, so the next day.
        assert!(doc.contains("DTEND;VALUE=DATE:20260323"), "{doc}");
    }

    #[test]
    fn summary_contains_iso_week_and_assignee() {
        let doc = ical_document("Baumhaus", "slug", &[vec![week(2026, 12, Some("Anna"))]]);
        assert!(doc.contains("SUMMARY:Kehrwoche KW 12: Anna"), "{doc}");
    }

    #[test]
    fn unassigned_weeks_get_a_fallback_summary() {
        let doc = ical_document("Baumhaus", "slug", &[vec![week(2026, 12, None)]]);
        assert!(
            doc.contains("SUMMARY:Kehrwoche KW 12: Nicht zugewiesen"),
            "{doc}"
        );
    }

    #[test]
    fn uid_is_stable_and_identifies_week_and_building() {
        let a = uid_for("slug", 2026, 12);
        let b = uid_for("slug", 2026, 12);
        let c = uid_for("slug", 2026, 13);
        let d = uid_for("other-slug", 2026, 12);
        assert_eq!(a, b, "uid must be stable across regenerations");
        assert_ne!(a, c, "different weeks must have different uids");
        assert_ne!(a, d, "different buildings must have different uids");
        assert_eq!(a, "kehrwoche-2026-12-slug@kehrkraft");
    }

    #[test]
    fn special_characters_are_escaped() {
        let escaped = escape_ical("Anna, die \\ \"Chefin\"; Müller\n");
        assert_eq!(escaped, "Anna\\, die \\\\ \"Chefin\"\\; Müller\\n");
    }

    #[test]
    fn short_lines_are_left_untouched_and_long_ones_unfold() {
        let short = "SUMMARY:Kehrwoche KW 12: Anna";
        assert_eq!(fold_line(short), short);

        let long = "Sehr langes Gebäude mit einem wirklich ausführlichen Namen, der die Zeilenlänge überschreitet";
        let folded = fold_line(long);
        assert_ne!(folded, long);
        // Unfold by stripping the CRLF+space continuations.
        let unfolded = folded.replace("\r\n ", "");
        assert_eq!(unfolded, long);
        for physical in folded.split("\r\n") {
            assert!(physical.len() <= 75, "physical line too long: {physical:?}");
        }
    }

    #[test]
    fn every_physical_line_of_the_document_is_at_most_75_octets() {
        let long_name = "Sehr langes Gebäude mit einem wirklich ausführlichen Namen";
        let mut schedules = Vec::new();
        for y in 2026..2028 {
            let weeks = if y == 2026 { 53 } else { 52 };
            schedules.push(
                (1..=weeks)
                    .map(|w| week(y, w, Some("Anna, die Chefin")))
                    .collect(),
            );
        }
        let doc = ical_document(long_name, "slug", &schedules);
        for line in doc.split("\r\n") {
            assert!(line.len() <= 75, "physical line too long: {line:?}");
        }
    }

    #[test]
    fn document_is_a_complete_vcalendar() {
        let doc = ical_document("Baumhaus", "slug", &[vec![week(2026, 12, Some("Anna"))]]);
        assert!(doc.starts_with("BEGIN:VCALENDAR\r\n"), "{doc}");
        assert!(doc.ends_with("END:VCALENDAR\r\n"), "{doc}");
        assert!(doc.contains("X-WR-CALNAME:Kehrwoche Baumhaus"), "{doc}");
        assert_eq!(doc.matches("BEGIN:VEVENT").count(), 1);
        assert_eq!(doc.matches("END:VEVENT").count(), 1);
    }
}
