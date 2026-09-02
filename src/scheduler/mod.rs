use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::db::models::{Ownership, Tenancy};
use crate::db::{queries, Db};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeekAssignment {
    pub year: i32,
    pub iso_week: u32,
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// id of the assigned owner or (when delegated) tenant
    pub assignee_id: Option<String>,
    pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
    /// true when the week's responsibility was delegated from owner to tenant
    pub delegated: bool,
}

#[derive(Debug)]
pub enum ScheduleError {
    Db(sqlx::Error),
    BuildingNotFound,
    DateParse(String),
}

impl From<sqlx::Error> for ScheduleError {
    fn from(e: sqlx::Error) -> Self {
        ScheduleError::Db(e)
    }
}

/// Common accessors for ownership and tenancy rows, which share a layout.
trait DatedRecord {
    fn id(&self) -> &str;
    fn apartment_id(&self) -> &str;
    fn name(&self) -> &str;
    fn email(&self) -> &str;
    fn start_date(&self) -> &str;
    fn end_date(&self) -> Option<&str>;
}

impl DatedRecord for Ownership {
    fn id(&self) -> &str {
        &self.id
    }
    fn apartment_id(&self) -> &str {
        &self.apartment_id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn email(&self) -> &str {
        &self.email
    }
    fn start_date(&self) -> &str {
        &self.start_date
    }
    fn end_date(&self) -> Option<&str> {
        self.end_date.as_deref()
    }
}

impl DatedRecord for Tenancy {
    fn id(&self) -> &str {
        &self.id
    }
    fn apartment_id(&self) -> &str {
        &self.apartment_id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn email(&self) -> &str {
        &self.email
    }
    fn start_date(&self) -> &str {
        &self.start_date
    }
    fn end_date(&self) -> Option<&str> {
        self.end_date.as_deref()
    }
}

#[derive(Clone, Debug)]
struct DatedPerson {
    id: String,
    apartment_id: String,
    name: String,
    email: String,
    start: NaiveDate,
    end: Option<NaiveDate>,
}

/// Compute weekly Kehrwoche assignments for a building and ISO year.
///
/// Rules (domain model):
/// - ISO weeks (Mon–Sun).
/// - Responsibility is assigned to the owners of the apartments round-robin.
///   Only ownership records covering the week participate: a period must
///   contain the whole week (start_date <= week_start AND (end_date IS NULL
///   OR end_date >= week_end)); apartments without an active owner are
///   skipped that week.
/// - Deterministic order: sort by (start_date asc, name asc); week offset =
///   (rotation_seed + week_index) % active_len.
/// - If the chosen apartment has a tenancy covering the week, the
///   responsibility is delegated to the tenant.
pub async fn schedule_for_year(
    building_id: &str,
    year: i32,
    pool: &Db,
) -> Result<Vec<WeekAssignment>, ScheduleError> {
    // Load building to get rotation_seed
    let building_opt = queries::get_building(pool, building_id).await?;
    let (building, _) = match building_opt {
        Some(b) => b,
        None => return Err(ScheduleError::BuildingNotFound),
    };
    let seed = building.rotation_seed;

    // Load owners and tenants of all apartments of the building
    let ownerships = queries::list_ownerships_for_building(pool, building_id).await?;
    let tenancies = queries::list_tenancies_for_building(pool, building_id).await?;
    let mut owners = parse_people(ownerships)?;
    let tenancies = parse_people(tenancies)?;

    // Deterministic base order for owners
    sort_people(&mut owners);

    // Iterate ISO weeks of the given year
    let mut weeks = Vec::new();
    // ISO week 1 Monday always exists
    let mut week_start = NaiveDate::from_isoywd_opt(year, 1, Weekday::Mon)
        .expect("valid ISO week start for given year");
    let mut week_index: usize = 0;

    while week_start.iso_week().year() == year {
        let week_end = week_start + Duration::days(6);

        // Active owners for the week, preserving the pre-sorted order
        let active: Vec<&DatedPerson> = owners
            .iter()
            .filter(|o| covers_week(o, week_start, week_end))
            .collect();

        let assignment = if active.is_empty() {
            WeekAssignment {
                year,
                iso_week: week_start.iso_week().week(),
                start: week_start,
                end: week_end,
                assignee_id: None,
                assignee_name: None,
                assignee_email: None,
                delegated: false,
            }
        } else {
            let active_len = active.len();
            let idx = ((seed as i128 + week_index as i128).rem_euclid(active_len as i128)) as usize;
            let chosen = active[idx];

            // Delegate to the tenant when a tenancy is active for the week.
            // (The app rejects overlapping tenancies, so at most one matches.)
            let delegated = tenancies.iter().find(|t| {
                t.apartment_id == chosen.apartment_id && covers_week(t, week_start, week_end)
            });

            let (assignee_id, assignee_name, assignee_email, is_delegated) = match delegated {
                Some(t) => (
                    Some(t.id.clone()),
                    Some(t.name.clone()),
                    Some(t.email.clone()),
                    true,
                ),
                None => (
                    Some(chosen.id.clone()),
                    Some(chosen.name.clone()),
                    Some(chosen.email.clone()),
                    false,
                ),
            };

            WeekAssignment {
                year,
                iso_week: week_start.iso_week().week(),
                start: week_start,
                end: week_end,
                assignee_id,
                assignee_name,
                assignee_email,
                delegated: is_delegated,
            }
        };

        weeks.push(assignment);

        week_start += Duration::days(7);
        week_index += 1;
    }

    Ok(weeks)
}

fn parse_people<P: DatedRecord>(raw: Vec<P>) -> Result<Vec<DatedPerson>, ScheduleError> {
    raw.into_iter()
        .map(|p| {
            let start = NaiveDate::parse_from_str(p.start_date(), "%Y-%m-%d").map_err(|_| {
                ScheduleError::DateParse(format!("invalid start_date {}", p.start_date()))
            })?;
            let end = match p.end_date() {
                Some(s) => Some(
                    NaiveDate::parse_from_str(s, "%Y-%m-%d")
                        .map_err(|_| ScheduleError::DateParse(format!("invalid end_date {}", s)))?,
                ),
                None => None,
            };
            Ok(DatedPerson {
                id: p.id().to_string(),
                apartment_id: p.apartment_id().to_string(),
                name: p.name().to_string(),
                email: p.email().to_string(),
                start,
                end,
            })
        })
        .collect()
}

fn sort_people(people: &mut [DatedPerson]) {
    people.sort_by(|a, b| match a.start.cmp(&b.start) {
        std::cmp::Ordering::Equal => a.name.cmp(&b.name),
        other => other,
    });
}

fn covers_week(p: &DatedPerson, week_start: NaiveDate, week_end: NaiveDate) -> bool {
    // The period must contain the whole week (Mon–Sun) so that nobody is
    // responsible before their period begins or after it ended (a tenancy
    // starting on 2026-02-01 must not cover the week 2026-01-26..2026-02-01).
    p.start <= week_start && p.end.map(|e| e >= week_end).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup() -> (Db, String) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");
        migrate(&pool).await.expect("migrate");
        let building =
            queries::create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
                .await
                .expect("create building");
        (pool, building.id)
    }

    async fn add_owner(pool: &Db, apartment_id: &str, name: &str, start: &str, end: Option<&str>) {
        queries::create_ownership(
            pool,
            apartment_id,
            name,
            &format!("{name}@example.com"),
            start,
            end,
        )
        .await
        .expect("create ownership");
    }

    async fn add_tenant(pool: &Db, apartment_id: &str, name: &str, start: &str, end: Option<&str>) {
        queries::create_tenancy(
            pool,
            apartment_id,
            name,
            &format!("{name}@example.com"),
            start,
            end,
        )
        .await
        .expect("create tenancy");
    }

    #[tokio::test]
    async fn single_owner_entire_year() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = queries::create_apartment(&pool, &building_id, "Apartment 1", "")
            .await
            .expect("create apartment");
        add_owner(&pool, &apt.id, "Alice", &format!("{year}-01-01"), None).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");
        // 52 or 53 ISO weeks
        assert!(schedule.len() >= 52 && schedule.len() <= 53);
        assert!(schedule
            .iter()
            .all(|w| w.assignee_name.as_deref() == Some("Alice") && !w.delegated));
    }

    #[tokio::test]
    async fn no_apartments_means_unassigned() {
        let (pool, building_id) = setup().await;

        let schedule = schedule_for_year(&building_id, 2024, &pool)
            .await
            .expect("schedule");
        assert!(schedule.len() >= 52);
        assert!(schedule.iter().all(|w| w.assignee_name.is_none()));
    }

    #[tokio::test]
    async fn round_robin_rotates_between_owners() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt1 = queries::create_apartment(&pool, &building_id, "A", "")
            .await
            .expect("create apartment");
        let apt2 = queries::create_apartment(&pool, &building_id, "B", "")
            .await
            .expect("create apartment");
        add_owner(&pool, &apt1.id, "Otto", &format!("{year}-01-01"), None).await;
        add_owner(&pool, &apt2.id, "Petra", &format!("{year}-01-01"), None).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        // With seed 0, weeks alternate: week 0 -> Otto, week 1 -> Petra, ...
        let otto_weeks = schedule
            .iter()
            .filter(|w| w.assignee_name.as_deref() == Some("Otto"))
            .count();
        assert!(otto_weeks > 0, "Otto should be assigned at least once");
        assert!(
            schedule
                .iter()
                .enumerate()
                .all(|(i, w)| (i % 2 == 0) == (w.assignee_name.as_deref() == Some("Otto"))),
            "assignees must alternate every week"
        );
        assert!(schedule.iter().all(|w| !w.delegated));
    }

    #[tokio::test]
    async fn active_tenancy_delegates_owner_duty() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = queries::create_apartment(&pool, &building_id, "A", "")
            .await
            .expect("create apartment");
        add_owner(&pool, &apt.id, "Otto", &format!("{year}-01-01"), None).await;
        // Tenancy from mid-February until the end of March
        let t_start = format!("{year}-02-15");
        let t_end = format!("{year}-03-31");
        add_tenant(&pool, &apt.id, "Melanie", &t_start, Some(&t_end)).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        let t_start_date = NaiveDate::parse_from_str(&t_start, "%Y-%m-%d").unwrap();
        let t_end_date = NaiveDate::parse_from_str(&t_end, "%Y-%m-%d").unwrap();

        // Weeks fully inside the tenancy are delegated to the tenant
        for w in schedule
            .iter()
            .filter(|w| w.start >= t_start_date && w.end <= t_end_date)
        {
            assert_eq!(w.assignee_name.as_deref(), Some("Melanie"));
            assert!(w.delegated, "week {} should be delegated", w.iso_week);
        }
        // Weeks outside the tenancy stay with the owner
        for w in schedule.iter().filter(|w| w.end < t_start_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("Otto"));
            assert!(!w.delegated);
        }
        for w in schedule.iter().filter(|w| w.start > t_end_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("Otto"));
            assert!(!w.delegated);
        }
    }

    #[tokio::test]
    async fn tenancy_without_end_delegates_until_year_end() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = queries::create_apartment(&pool, &building_id, "A", "")
            .await
            .expect("create apartment");
        add_owner(&pool, &apt.id, "Otto", &format!("{year}-01-01"), None).await;
        let t_start = format!("{year}-06-01");
        add_tenant(&pool, &apt.id, "Nina", &t_start, None).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        let t_start_date = NaiveDate::parse_from_str(&t_start, "%Y-%m-%d").unwrap();
        for w in schedule.iter().filter(|w| w.start >= t_start_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("Nina"));
            assert!(w.delegated);
        }
    }

    #[tokio::test]
    async fn tenancy_does_not_start_before_its_begin() {
        let (pool, building_id) = setup().await;
        let year = 2026;
        let apt = queries::create_apartment(&pool, &building_id, "A", "")
            .await
            .expect("create apartment");
        add_owner(&pool, &apt.id, "Otto", &format!("{year}-01-01"), None).await;
        // Tenancy begins 2026-02-01 (a Sunday); the week 2026-01-26..2026-02-01
        // must stay with the owner and only the following week be delegated.
        add_tenant(&pool, &apt.id, "Nora", &format!("{year}-02-01"), None).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        let week_before = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2026, 1, 26).unwrap())
            .expect("week 2026-01-26");
        assert_eq!(week_before.assignee_name.as_deref(), Some("Otto"));
        assert!(!week_before.delegated);

        let week_after = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2026, 2, 2).unwrap())
            .expect("week 2026-02-02");
        assert_eq!(week_after.assignee_name.as_deref(), Some("Nora"));
        assert!(week_after.delegated);
    }

    #[tokio::test]
    async fn gaps_and_year_boundaries() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = queries::create_apartment(&pool, &building_id, "A", "")
            .await
            .expect("create apartment");

        // Owner A: owns until mid-January
        let a_start = format!("{year}-01-01");
        let a_end = format!("{year}-01-15");
        add_owner(&pool, &apt.id, "A", &a_start, Some(&a_end)).await;

        // Owner B: owns from February on
        let b_start = format!("{year}-02-01");
        add_owner(&pool, &apt.id, "B", &b_start, None).await;

        let a_end_date = NaiveDate::parse_from_str(&a_end, "%Y-%m-%d").unwrap();
        let b_start_date = NaiveDate::parse_from_str(&b_start, "%Y-%m-%d").unwrap();

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        // Weeks up to A's end should be assigned to A (B not yet owner)
        for w in schedule.iter().filter(|w| w.end <= a_end_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("A"));
        }

        // At least one unassigned week between A's end and B's start
        let gap_exists = schedule
            .iter()
            .any(|w| w.start > a_end_date && w.end < b_start_date && w.assignee_name.is_none());
        assert!(
            gap_exists,
            "Expected at least one unassigned week between A end and B start"
        );

        // Weeks starting at or after B's start should be assigned to B
        for w in schedule.iter().filter(|w| w.start >= b_start_date) {
            assert_eq!(w.assignee_name.as_deref(), Some("B"));
        }
    }
}
