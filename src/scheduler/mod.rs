use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::db::{queries, Db};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeekAssignment {
    pub year: i32,
    pub iso_week: u32,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub assignee_tenant_id: Option<String>,
    pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
}

#[derive(Debug)]
pub enum ScheduleError {
    Db(sqlx::Error),
    PlanNotFound,
    DateParse(String),
}

impl From<sqlx::Error> for ScheduleError {
    fn from(e: sqlx::Error) -> Self {
        ScheduleError::Db(e)
    }
}

#[derive(Clone, Debug)]
struct TenantParsed {
    id: String,
    name: String,
    email: String,
    start: NaiveDate,
    end: Option<NaiveDate>,
}

/// Compute weekly Kehrwoche assignments for a plan and ISO year.
/// Rules:
/// - ISO weeks (Mon–Sun)
/// - Active tenants for a week satisfy: start_date <= week_end AND (end_date IS NULL OR end_date >= week_start)
/// - Deterministic order: sort by (start_date asc, name asc)
/// - Rotation: offset = (rotation_seed + week_index) % active_len
pub async fn schedule_for_year(
    plan_id: &str,
    year: i32,
    pool: &Db,
) -> Result<Vec<WeekAssignment>, ScheduleError> {
    // Load plan to get rotation_seed
    let plan_opt = queries::get_plan(pool, plan_id).await?;
    let (plan, _) = match plan_opt {
        Some(p) => p,
        None => return Err(ScheduleError::PlanNotFound),
    };
    let seed = plan.rotation_seed;

    // Load tenants for plan
    let tenants = queries::list_tenants(pool, plan_id).await?;
    let mut tenants = parse_tenants(tenants)?;

    // Deterministic base order for tenants
    tenants.sort_by(|a, b| {
        match a.start.cmp(&b.start) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            other => other,
        }
    });

    // Iterate ISO weeks of the given year
    let mut weeks = Vec::new();
    // ISO week 1 Monday always exists
    let mut week_start = NaiveDate::from_isoywd_opt(year, 1, Weekday::Mon)
        .expect("valid ISO week start for given year");
    let mut week_index: usize = 0;

    while week_start.iso_week().year() == year {
        let week_end = week_start + Duration::days(6);

        // Filter active tenants, preserving the pre-sorted order
        let active: Vec<&TenantParsed> = tenants
            .iter()
            .filter(|t| is_active_for_week(t, week_start, week_end))
            .collect();

        if active.is_empty() {
            weeks.push(WeekAssignment {
                year,
                iso_week: week_start.iso_week().week(),
                start: week_start,
                end: week_end,
                assignee_tenant_id: None,
                assignee_name: None,
                assignee_email: None,
            });
        } else {
            let active_len = active.len();
            let idx =
                ((seed as i128 + week_index as i128).rem_euclid(active_len as i128)) as usize;
            let chosen = active[idx];

            weeks.push(WeekAssignment {
                year,
                iso_week: week_start.iso_week().week(),
                start: week_start,
                end: week_end,
                assignee_tenant_id: Some(chosen.id.clone()),
                assignee_name: Some(chosen.name.clone()),
                assignee_email: Some(chosen.email.clone()),
            });
        }

        week_start = week_start + Duration::days(7);
        week_index += 1;
    }

    Ok(weeks)
}

fn parse_tenants(raw: Vec<crate::db::models::Tenant>) -> Result<Vec<TenantParsed>, ScheduleError> {
    let mut out = Vec::with_capacity(raw.len());
    for t in raw {
        let start = NaiveDate::parse_from_str(&t.start_date, "%Y-%m-%d")
            .map_err(|_| ScheduleError::DateParse(format!("invalid start_date {}", t.start_date)))?;
        let end = match t.end_date {
            Some(s) => Some(
                NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .map_err(|_| ScheduleError::DateParse(format!("invalid end_date {}", s)))?,
            ),
            None => None,
        };
        out.push(TenantParsed {
            id: t.id,
            name: t.name,
            email: t.email,
            start,
            end,
        });
    }
    Ok(out)
}

fn is_active_for_week(t: &TenantParsed, week_start: NaiveDate, week_end: NaiveDate) -> bool {
    t.start <= week_end && t.end.map(|e| e >= week_start).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn single_tenant_entire_year() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        // Create plan and a single tenant active the whole year
        let plan = queries::create_plan(&pool, "Test Plan", "Alice Admin", "alice.admin@example.com")
            .await
            .expect("create plan");

        let year = 2024;
        let start_date = format!("{year}-01-01");
        queries::create_tenant(
            &pool,
            &plan.id,
            "Alice",
            "alice@example.com",
            &start_date,
            None,
        )
        .await
        .expect("create tenant");

        let schedule = schedule_for_year(&plan.id, year, &pool)
            .await
            .expect("schedule");
        // 52 or 53 ISO weeks
        assert!(schedule.len() >= 52 && schedule.len() <= 53);
        assert!(schedule.iter().all(|w| w.assignee_name.as_deref() == Some("Alice")));
    }

    #[tokio::test]
    async fn gaps_and_year_boundaries() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");
        migrate(&pool).await.expect("migrate");

        let plan = queries::create_plan(&pool, "Boundary Plan", "Bob Admin", "bob.admin@example.com")
            .await
            .expect("create plan");

        let year = 2024;

        // Tenant A: active until mid-January
        let a_start = format!("{year}-01-01");
        let a_end = format!("{year}-01-15");
        queries::create_tenant(
            &pool,
            &plan.id,
            "A",
            "a@example.com",
            &a_start,
            Some(&a_end),
        )
        .await
        .expect("create tenant A");

        // Tenant B: starts in February
        let b_start = format!("{year}-02-01");
        queries::create_tenant(
            &pool,
            &plan.id,
            "B",
            "b@example.com",
            &b_start,
            None,
        )
        .await
        .expect("create tenant B");

        let a_end_date = NaiveDate::parse_from_str(&a_end, "%Y-%m-%d").unwrap();
        let b_start_date = NaiveDate::parse_from_str(&b_start, "%Y-%m-%d").unwrap();

        let schedule = schedule_for_year(&plan.id, year, &pool)
            .await
            .expect("schedule");

        // Weeks up to A's end should be assigned to A (B not yet active)
        for w in schedule.iter().filter(|w| w.end <= a_end_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("A"));
        }

        // There should be at least one unassigned week between A end and B start
        let gap_exists = schedule.iter().any(|w| {
            w.start > a_end_date && w.end < b_start_date && w.assignee_name.is_none()
        });
        assert!(gap_exists, "Expected at least one unassigned week between A end and B start");

        // Weeks starting at or after B start should be assigned to B
        for w in schedule.iter().filter(|w| w.start >= b_start_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("B"));
        }
    }
}
