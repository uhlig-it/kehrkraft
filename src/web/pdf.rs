use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
};
use chrono::{Datelike, Local};
use rand::RngCore;
use std::path::PathBuf;
use tokio::{
    fs,
    time::{timeout, Duration},
};
use tokio::process::Command;

use crate::db::{queries, Db};
use crate::scheduler;

fn escape_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else if c.is_ascii_whitespace() {
                '-'
            } else {
                '-'
            }
        })
        .collect()
}

pub async fn public_pdf(Path(secret_slug): Path<String>, State(pool): State<Db>) -> impl IntoResponse {
    // Lookup plan by secret slug
    let plan = match queries::get_plan_by_slug(&pool, &secret_slug).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, "Plan not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    // Compute current year's schedule
    let year = Local::now().year();
    let schedule = match scheduler::schedule_for_year(&plan.id, year, &pool).await {
        Ok(s) => s,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to compute schedule").into_response(),
    };

    // Prepare a per-request temp directory
    let mut rnd_bytes = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut rnd_bytes);
    let rnd = u64::from_le_bytes(rnd_bytes);
    let tmp_dir: PathBuf = std::env::temp_dir().join(format!("kehrkraft-{}-{}", std::process::id(), rnd));

    if let Err(_) = fs::create_dir_all(&tmp_dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create temp dir").into_response();
    }

    // Write Typst template into temp dir
    let template = include_str!("../../assets/typst/kehrwoche.typ");
    if let Err(_) = fs::write(tmp_dir.join("kehrwoche.typ"), template).await {
        let _ = fs::remove_dir_all(&tmp_dir).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write template").into_response();
    }

    // Build rows for Typst
    let rows_parts: Vec<String> = schedule
        .iter()
        .map(|w| {
            let name = escape_typst_str(w.assignee_name.as_deref().unwrap_or(""));
            let email = escape_typst_str(w.assignee_email.as_deref().unwrap_or(""));
            format!(
                "(week: {}, start: \"{}\", end: \"{}\", name: \"{}\", email: \"{}\")",
                w.iso_week,
                w.start.format("%Y-%m-%d"),
                w.end.format("%Y-%m-%d"),
                name,
                email
            )
        })
        .collect();

    let rows_src = format!("[\n{}\n]", rows_parts.join(",\n"));

    // Build wrapper Typst source
    let plan_name_escaped = escape_typst_str(&plan.name);
    let wrapper_src = format!(
        r#"#import "kehrwoche.typ": kehrwoche

#let plan_name = "{plan_name}"
#let year = {year}
#let rows = {rows}

#kehrwoche(plan_name: plan_name, year: year, rows: rows)
"#,
        plan_name = plan_name_escaped,
        year = year,
        rows = rows_src,
    );

    if let Err(_) = fs::write(tmp_dir.join("wrapper.typ"), wrapper_src.as_bytes()).await {
        let _ = fs::remove_dir_all(&tmp_dir).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write Typst source").into_response();
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
        tracing::error!(
            status = ?output.status,
            %stderr,
            %stdout,
            "Typst compile failed"
        );
        let _ = fs::remove_dir_all(&tmp_dir).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, "Typst compile failed").into_response();
    }

    // Read PDF bytes
    let pdf_path = tmp_dir.join("out.pdf");
    let pdf_bytes = match fs::read(&pdf_path).await {
        Ok(b) => b,
        Err(_) => {
            let _ = fs::remove_dir_all(&tmp_dir).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "PDF not found after compile").into_response();
        }
    };

    // Cleanup temp dir
    let _ = fs::remove_dir_all(&tmp_dir).await;

    // Build response with headers
    let safe_plan = sanitize_filename(&plan.name);
    let filename = format!("Kehrwoche-{}-{}.pdf", safe_plan, year);
    let cd_val = format!("attachment; filename=\"{}\"", filename);
    let cd = HeaderValue::from_str(&cd_val).unwrap_or_else(|_| HeaderValue::from_static("attachment"));

    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("application/pdf")),
            (header::CONTENT_DISPOSITION, cd),
        ],
        pdf_bytes,
    )
        .into_response()
}
