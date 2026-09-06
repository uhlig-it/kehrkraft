use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
};
use chrono::{Datelike, Local};
use fast_qr::convert::svg::SvgBuilder;
use fast_qr::qr::QRBuilder;
use rand::RngCore;
use std::path::PathBuf;
use tokio::process::Command;
use tokio::{
    fs,
    time::{timeout, Duration},
};

use crate::db::{queries, Db};
use crate::scheduler::{self, WeekAssignment};

const KEHRKRAFT_SVG: &[u8] = include_bytes!("../../kehrkraft.svg");

fn escape_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Typst source for one column of the schedule table. `info` is true for rows
/// that are not part of `year` (shown greyed out; see the template).
fn rows_source(year: i32, rows: &[&WeekAssignment]) -> String {
    let parts: Vec<String> = rows
        .iter()
        .map(|w| {
            let name = escape_typst_str(w.assignee_name.as_deref().unwrap_or(""));
            format!(
                "(week: {}, start: \"{}\", end: \"{}\", name: \"{}\", info: {})",
                w.iso_week,
                w.start.format("%d.%m.%y"),
                w.end.format("%d.%m.%y"),
                name,
                w.year != year
            )
        })
        .collect();
    format!("(\n{}\n)", parts.join(",\n"))
}

/// Split a year's schedule into two equally sized columns.
///
/// The current year is split as evenly as possible. Years with an odd number
/// of ISO weeks (53-week years) cannot be split exactly, so — mirroring the
/// reference Kehrwoche.ods layout — the last week of the previous year is shown
/// greyed out at the top of the left column and the first two weeks of the
/// next year at the bottom of the right column, making both columns the same
/// length.
fn balanced_columns<'a>(
    prev: &'a [WeekAssignment],
    current: &'a [WeekAssignment],
    next: &'a [WeekAssignment],
) -> (Vec<&'a WeekAssignment>, Vec<&'a WeekAssignment>) {
    let n = current.len();
    if n.is_multiple_of(2) {
        let (left, right) = current.split_at(n / 2);
        return (left.iter().collect(), right.iter().collect());
    }

    // Total rows become n + 3 (one grey previous-year row, two grey
    // next-year rows); both columns end up with the same row count.
    let rows_per_column = (n + 3) / 2;
    let left_current = rows_per_column - 1;

    let mut left = Vec::with_capacity(rows_per_column);
    left.push(
        prev.last()
            .expect("an ISO year always has at least one week"),
    );
    left.extend(current[..left_current].iter());

    let mut right = Vec::with_capacity(rows_per_column);
    right.extend(current[left_current..].iter());
    right.extend(next[..2].iter());

    (left, right)
}

/// Absolute link for the QR code on the PDF; empty when no public URL is
/// configured (the template then renders a placeholder instead of a QR code).
fn pdf_url_for(public_url: Option<&str>, secret_slug: &str) -> String {
    public_url
        .map(|base| format!("{}/p/{}/kehrwoche.pdf", base, secret_slug))
        .unwrap_or_default()
}

pub async fn public_pdf(
    Path(secret_slug): Path<String>,
    State(pool): State<Db>,
    State(public_url): State<Option<String>>,
) -> impl IntoResponse {
    // Lookup building by secret slug
    let building = match queries::get_building_by_slug(&pool, &secret_slug).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Building not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    // Compute the schedules surrounding the current year: the neighboring
    // years provide the grey informational rows that balance the columns.
    let year = Local::now().year();
    let schedule = match scheduler::schedule_for_year(&building.id, year, &pool).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to compute schedule",
            )
                .into_response()
        }
    };
    let prev_schedule = match scheduler::schedule_for_year(&building.id, year - 1, &pool).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to compute schedule",
            )
                .into_response()
        }
    };
    let next_schedule = match scheduler::schedule_for_year(&building.id, year + 1, &pool).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to compute schedule",
            )
                .into_response()
        }
    };

    let (left_rows, right_rows) = balanced_columns(&prev_schedule, &schedule, &next_schedule);

    // Absolute link for the QR code; empty when no public URL is configured,
    // in which case the template renders a placeholder instead of a QR code.
    let mut pdf_url = pdf_url_for(public_url.as_deref(), &secret_slug);

    // Prepare a per-request temp directory
    let mut rnd_bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut rnd_bytes);
    let rnd = u64::from_le_bytes(rnd_bytes);
    let tmp_dir: PathBuf =
        std::env::temp_dir().join(format!("kehrkraft-{}-{}", std::process::id(), rnd));

    if fs::create_dir_all(&tmp_dir).await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create temp dir",
        )
            .into_response();
    }

    // Write Typst template and the logo (lives next to the template so the
    // compile below can resolve it with a relative path).
    let template = include_str!("../../assets/typst/kehrwoche.typ");
    if fs::write(tmp_dir.join("kehrwoche.typ"), template)
        .await
        .is_err()
        || fs::write(tmp_dir.join("logo.svg"), KEHRKRAFT_SVG)
            .await
            .is_err()
    {
        let _ = fs::remove_dir_all(&tmp_dir).await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to write template",
        )
            .into_response();
    }

    // Generate the QR code pointing at the PDF itself.
    if !pdf_url.is_empty() {
        match QRBuilder::new(pdf_url.as_str()).build() {
            Ok(qr) => {
                let svg = SvgBuilder::default().to_str(&qr);
                if fs::write(tmp_dir.join("qr.svg"), svg.as_bytes())
                    .await
                    .is_err()
                {
                    let _ = fs::remove_dir_all(&tmp_dir).await;
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write QR code")
                        .into_response();
                }
            }
            Err(err) => {
                tracing::warn!(%err, "QR code generation failed; rendering the PDF without a QR code");
                pdf_url.clear();
            }
        }
    }

    // Build wrapper Typst source
    let building_name_escaped = escape_typst_str(&building.name);
    let left_rows_src = rows_source(year, &left_rows);
    let right_rows_src = rows_source(year, &right_rows);
    // Version of the running binary, baked in at compile time from Cargo.toml
    let version = env!("CARGO_PKG_VERSION");
    let wrapper_src = format!(
        r#"#import "kehrwoche.typ": kehrwoche

#set page(
  paper: "a4",
  margin: (top: 1.7cm, bottom: 1.5cm, x: 1.6cm),
  footer: [
    #set text(8pt)
    #columns(2)[
      #set align(left)
      Erstellt mit Kehrkraft v{version}
      #colbreak()
      #set align(right)
      Stand: #datetime.today().display("[day].[month].[year]")
    ]
  ]
)

#let building_name = "{building_name}"
#let year = {year}
#let left_rows = {left_rows}
#let right_rows = {right_rows}
#let pdf_url = "{pdf_url}"

#kehrwoche(
  building_name: building_name,
  year: year,
  left_rows: left_rows,
  right_rows: right_rows,
  pdf_url: pdf_url,
)
"#,
        building_name = building_name_escaped,
        year = year,
        left_rows = left_rows_src,
        right_rows = right_rows_src,
        pdf_url = pdf_url,
        version = version,
    );

    if fs::write(tmp_dir.join("wrapper.typ"), wrapper_src.as_bytes())
        .await
        .is_err()
    {
        let _ = fs::remove_dir_all(&tmp_dir).await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to write Typst source",
        )
            .into_response();
    }

    // Run typst compile with a timeout
    let compile = Command::new("typst")
        .arg("compile")
        .arg("wrapper.typ")
        .arg("out.pdf")
        .current_dir(&tmp_dir)
        .output();

    let output = match timeout(Duration::from_secs(20), compile).await {
        Ok(Ok(out)) => out,
        Ok(Err(_)) => {
            let _ = fs::remove_dir_all(&tmp_dir).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to execute typst").into_response();
        }
        Err(_) => {
            let _ = fs::remove_dir_all(&tmp_dir).await;
            return (StatusCode::GATEWAY_TIMEOUT, "Typst compile timed out").into_response();
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let wrapper = tmp_dir.join("wrapper.typ");
        let template = tmp_dir.join("kehrwoche.typ");
        let pdf_out = tmp_dir.join("out.pdf");
        tracing::error!(
            status = ?output.status,
            %stderr,
            %stdout,
            tmp_dir = %tmp_dir.display(),
            wrapper = %wrapper.display(),
            template = %template.display(),
            pdf_out = %pdf_out.display(),
            "Typst compile failed; preserving temp files for inspection"
        );
        return (StatusCode::INTERNAL_SERVER_ERROR, "Typst compile failed").into_response();
    }

    // Read PDF bytes
    let pdf_path = tmp_dir.join("out.pdf");
    let pdf_bytes = match fs::read(&pdf_path).await {
        Ok(b) => b,
        Err(_) => {
            let _ = fs::remove_dir_all(&tmp_dir).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "PDF not found after compile",
            )
                .into_response();
        }
    };

    // Cleanup temp dir
    let _ = fs::remove_dir_all(&tmp_dir).await;

    // Build response with headers
    let safe_building = sanitize_filename(&building.name);
    let filename = format!("Kehrwoche-{}-{}.pdf", safe_building, year);
    let cd_val = format!("inline; filename=\"{}\"", filename);
    let cd = HeaderValue::from_str(&cd_val).unwrap_or_else(|_| HeaderValue::from_static("inline"));

    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/pdf"),
            ),
            (header::CONTENT_DISPOSITION, cd),
        ],
        pdf_bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn week(year: i32, iso_week: u32) -> WeekAssignment {
        let start = NaiveDate::from_isoywd_opt(year, iso_week, chrono::Weekday::Mon)
            .expect("valid ISO date");
        WeekAssignment {
            year,
            iso_week,
            start,
            end: start + chrono::Duration::days(6),
            assignee_id: None,
            assignee_name: None,
            assignee_email: None,
            delegated: false,
        }
    }

    fn y2025() -> Vec<WeekAssignment> {
        (1..=52).map(|w| week(2025, w)).collect()
    }

    fn y2026() -> Vec<WeekAssignment> {
        (1..=53).map(|w| week(2026, w)).collect()
    }

    fn y2027() -> Vec<WeekAssignment> {
        (1..=52).map(|w| week(2027, w)).collect()
    }

    #[test]
    fn even_year_splits_exactly_in_half() {
        let current: Vec<WeekAssignment> = (1..=52).map(|w| week(2026, w)).collect();
        let prev = y2025();
        let next = y2027();
        let (left, right) = balanced_columns(&prev, &current, &next);

        assert_eq!(left.len(), 26);
        assert_eq!(right.len(), 26);
        assert_eq!(left[0].iso_week, 1);
        assert_eq!(left[25].iso_week, 26);
        assert_eq!(right[0].iso_week, 27);
        assert_eq!(right[25].iso_week, 52);
        // No cross-year padding rows.
        assert!(left.iter().chain(&right).all(|w| w.year == 2026));
    }

    #[test]
    fn odd_year_balances_with_grey_rows() {
        let prev = y2025();
        let current = y2026(); // 53 weeks
        let next = y2027();
        let (left, right) = balanced_columns(&prev, &current, &next);

        // Both columns have the same number of rows.
        assert_eq!(left.len(), 28);
        assert_eq!(right.len(), 28);

        // Left column: last week of the previous year, then weeks 1..=27.
        assert_eq!(left[0].iso_week, 52);
        assert_eq!(left[0].year, 2025);
        assert_eq!(left[1].iso_week, 1);
        assert_eq!(left[1].year, 2026);
        assert_eq!(left[27].iso_week, 27);

        // Right column: weeks 28..=53, then the first two weeks of next year.
        assert_eq!(right[0].iso_week, 28);
        assert_eq!(right.len(), 28);
        assert_eq!(right[25].iso_week, 53);
        assert_eq!(right[26].iso_week, 1);
        assert_eq!(right[26].year, 2027);
        assert_eq!(right[27].iso_week, 2);
        assert_eq!(right[27].year, 2027);
    }

    #[test]
    fn every_current_year_week_appears_exactly_once() {
        let prev = y2025();
        let current = y2026();
        let next = y2027();
        let (left, right) = balanced_columns(&prev, &current, &next);
        let all: Vec<&WeekAssignment> = left.into_iter().chain(right).collect();
        let current_rows: Vec<u32> = all
            .iter()
            .filter(|w| w.year == 2026)
            .map(|w| w.iso_week)
            .collect();
        assert_eq!(current_rows, (1..=53).collect::<Vec<u32>>());
    }

    #[test]
    fn rows_source_marks_cross_year_rows_as_informational() {
        let normal = week(2026, 5);
        let grey = week(2027, 1);
        let src = rows_source(2026, &[&normal, &grey]);
        assert!(src.contains("week: 5") && src.contains("info: false"));
        assert!(src.contains("week: 1") && src.contains("info: true"));
    }

    #[test]
    fn pdf_url_is_built_from_public_url_and_slug() {
        assert_eq!(
            pdf_url_for(Some("https://kehrkraft.uhlig.it"), "abc_123"),
            "https://kehrkraft.uhlig.it/p/abc_123/kehrwoche.pdf"
        );
        assert_eq!(pdf_url_for(None, "abc_123"), "");
    }
}
