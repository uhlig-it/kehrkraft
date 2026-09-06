use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::db::models::{Apartment, Ownership, Tenancy};
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

/// Global rotation index of an ISO week: weeks since the fixed Monday epoch
/// 2020-01-06 (ISO week 2 of 2020). The epoch never changes, so the rotation
/// continues seamlessly across years and the +1 imbalance of years with 53
/// weeks rotates between the apartments instead of always hitting the same
/// ones. `week_start` is always a Monday, so the day count is a multiple of 7.
fn global_week_index(week_start: NaiveDate) -> i64 {
    let epoch = NaiveDate::from_ymd_opt(2020, 1, 6).expect("valid rotation epoch");
    week_start.signed_duration_since(epoch).num_days() / 7
}

/// Days of the week [week_start, week_end] inside the person's period.
fn overlap_days(p: &DatedPerson, week_start: NaiveDate, week_end: NaiveDate) -> i64 {
    let start = p.start.max(week_start);
    let end = p.end.unwrap_or(week_end).min(week_end);
    if start > end {
        0
    } else {
        (end - start).num_days() + 1
    }
}

/// The owner whose period covers most of the week. Ownership periods tile an
/// apartment's timeline (enforced on create/update/delete), so from the first
/// period on exactly one owner covers at least 4 of the 7 days and every week
/// is resolved; a week overlapping the chain by fewer than half its days (the
/// apartment's first, partially covered week) yields `None` and the apartment
/// joins the rotation one week later. Ties cannot occur with tiled data; the
/// earliest period wins defensively.
fn majority_owner<'a>(
    owners: &[&'a DatedPerson],
    week_start: NaiveDate,
    week_end: NaiveDate,
) -> Option<&'a DatedPerson> {
    let mut best: Option<(&'a DatedPerson, i64)> = None;
    for owner in owners {
        let days = overlap_days(owner, week_start, week_end);
        if days >= 4 && best.is_none_or(|(_, best_days)| days > best_days) {
            best = Some((owner, days));
        }
    }
    best.map(|(owner, _)| owner)
}

/// Compute weekly Kehrwoche assignments for a building and ISO year.
///
/// Rules (domain model):
/// - ISO weeks (Mon–Sun).
/// - Responsibility is assigned round-robin over the *apartments* of the
///   building, in the order the apartments were created (immutable, so
///   owner and tenant changes never shift the duty weeks of other
///   apartments). The rotation counter is global and continuous across
///   years (see `global_week_index`); `rotation_seed` is the base offset.
/// - Every week has an assignee once at least one apartment has an owner:
///   the week belongs to the apartment's owner covering most of it, so
///   transition weeks between two owners are always resolved. Apartments
///   join the rotation with the first week that is at least half covered;
///   apartments without any covering ownership are skipped that week.
/// - If the chosen apartment has a tenancy covering the whole week, the
///   responsibility is delegated to the tenant (whole-week rule, so a
///   tenant is never responsible before their tenancy begins).
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

    // Load owners, tenants, and apartments of the building
    let ownerships = queries::list_ownerships_for_building(pool, building_id).await?;
    let tenancies = queries::list_tenancies_for_building(pool, building_id).await?;
    let owners = parse_people(ownerships)?;
    let tenancies = parse_people(tenancies)?;

    // Immutable rotation order: apartment creation order, id as tie-break
    // (creation timestamps have second granularity).
    let mut apartments = queries::list_apartments(pool, building_id).await?;
    apartments.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut owners_by_apartment: HashMap<&str, Vec<&DatedPerson>> = HashMap::new();
    for owner in &owners {
        owners_by_apartment
            .entry(owner.apartment_id.as_str())
            .or_default()
            .push(owner);
    }

    // Iterate ISO weeks of the given year
    let mut weeks = Vec::new();
    // ISO week 1 Monday always exists
    let mut week_start = NaiveDate::from_isoywd_opt(year, 1, Weekday::Mon)
        .expect("valid ISO week start for given year");

    while week_start.iso_week().year() == year {
        let week_end = week_start + Duration::days(6);

        // Apartments with an owner for this week, in rotation order
        let covered: Vec<(&Apartment, &DatedPerson)> = apartments
            .iter()
            .filter_map(|apartment| {
                let owner = owners_by_apartment
                    .get(apartment.id.as_str())
                    .and_then(|owners| majority_owner(owners, week_start, week_end))?;
                Some((apartment, owner))
            })
            .collect();

        let assignment = if covered.is_empty() {
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
            let covered_len = covered.len();
            let idx = ((seed as i128 + global_week_index(week_start) as i128)
                .rem_euclid(covered_len as i128)) as usize;
            let (chosen_apartment, chosen_owner) = covered[idx];

            // Delegate to the tenant when a tenancy covers the whole week.
            // (The app rejects overlapping tenancies, so at most one matches.)
            let delegated = tenancies.iter().find(|t| {
                t.apartment_id == chosen_apartment.id && covers_week(t, week_start, week_end)
            });

            let (assignee_id, assignee_name, assignee_email, is_delegated) = match delegated {
                Some(t) => (
                    Some(t.id.clone()),
                    Some(t.name.clone()),
                    Some(t.email.clone()),
                    true,
                ),
                None => (
                    Some(chosen_owner.id.clone()),
                    Some(chosen_owner.name.clone()),
                    Some(chosen_owner.email.clone()),
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
    use crate::db::models::Apartment;
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

    async fn add_apartment(
        pool: &Db,
        building_id: &str,
        name: &str,
        created_at: &str,
        owner_name: &str,
        owner_start: &str,
        owner_end: Option<&str>,
    ) -> Apartment {
        let owner_email = format!("{owner_name}@example.com");
        let apartment = queries::create_apartment(
            pool,
            building_id,
            name,
            "",
            &queries::NewOwner {
                name: owner_name,
                email: &owner_email,
                start_date: owner_start,
                end_date: owner_end,
            },
        )
        .await
        .expect("create apartment");
        // Fix the creation timestamp so the rotation order is deterministic.
        sqlx::query("UPDATE apartments SET created_at = ? WHERE id = ?")
            .bind(created_at)
            .bind(&apartment.id)
            .execute(pool)
            .await
            .expect("set created_at");
        apartment
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
        let _apt = add_apartment(
            &pool,
            &building_id,
            "Apartment 1",
            "2024-01-01 00:00:00",
            "Alice",
            "2024-01-01",
            None,
        )
        .await;

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
    async fn round_robin_rotates_between_apartments() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let _apt1 = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Otto",
            "2024-01-01",
            None,
        )
        .await;
        let _apt2 = add_apartment(
            &pool,
            &building_id,
            "B",
            "2024-01-02 00:00:00",
            "Petra",
            "2024-01-01",
            None,
        )
        .await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        // The global week index of 2024 week 1 is 208 (even), so with seed 0
        // the apartments alternate starting with apartment A (Otto).
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

    /// A positive `rotation_seed` shifts the rotation circle: with seed 1 the
    /// second apartment (creation order) starts the alternation instead of the
    /// first, moving every duty week one position.
    #[tokio::test]
    async fn rotation_seed_shifts_the_rotation() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let _apt1 = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Otto",
            "2024-01-01",
            None,
        )
        .await;
        let _apt2 = add_apartment(
            &pool,
            &building_id,
            "B",
            "2024-01-02 00:00:00",
            "Petra",
            "2024-01-01",
            None,
        )
        .await;

        queries::update_building_rotation_seed(&pool, &building_id, 1)
            .await
            .expect("set rotation seed");

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        // Seed 0 lets Otto (apartment A) take the even positions; seed 1 flips
        // the alternation so Petra (apartment B) starts.
        assert!(
            schedule
                .iter()
                .enumerate()
                .all(|(i, w)| { (i % 2 == 0) == (w.assignee_name.as_deref() == Some("Petra")) }),
            "seed 1 must shift the rotation by one position"
        );
    }

    /// The +1 imbalance of a 52-week year over 3 apartments (18 vs 17 weeks)
    /// must rotate between the apartments across years: the rotation counter
    /// continues globally instead of restarting at the same offset each year.
    #[tokio::test]
    async fn rotation_continues_across_years() {
        let (pool, building_id) = setup().await;
        let _apt1 = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Alice",
            "2024-01-01",
            None,
        )
        .await;
        let _apt2 = add_apartment(
            &pool,
            &building_id,
            "B",
            "2024-01-02 00:00:00",
            "Bob",
            "2024-01-01",
            None,
        )
        .await;
        let _apt3 = add_apartment(
            &pool,
            &building_id,
            "C",
            "2024-01-03 00:00:00",
            "Carol",
            "2024-01-01",
            None,
        )
        .await;

        let s2024 = schedule_for_year(&building_id, 2024, &pool)
            .await
            .expect("2024");
        let s2025 = schedule_for_year(&building_id, 2025, &pool)
            .await
            .expect("2025");

        let count = |s: &[WeekAssignment], name: &str| {
            s.iter()
                .filter(|w| w.assignee_name.as_deref() == Some(name))
                .count()
        };

        // 2024 (52 weeks): 208..259, residue 1 -> Bob gets the +1 (18 weeks).
        assert_eq!(s2024[0].assignee_name.as_deref(), Some("Bob"));
        assert_eq!(count(&s2024, "Bob"), 18);
        assert_eq!(count(&s2024, "Carol"), 17);
        // 2025 (52 weeks): 260..311, residue 2 -> Carol gets the +1.
        assert_eq!(s2025[0].assignee_name.as_deref(), Some("Carol"));
        assert_eq!(count(&s2025, "Bob"), 17);
        assert_eq!(count(&s2025, "Carol"), 18);
    }

    /// A mid-year owner handover must not shift the duty weeks of any
    /// apartment: only the person covering apartment B's weeks changes.
    #[tokio::test]
    async fn owner_handover_mid_year_keeps_apartment_weeks() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let _apt_a = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Alice",
            "2024-01-01",
            None,
        )
        .await;
        let apt_b = add_apartment(
            &pool,
            &building_id,
            "B",
            "2024-01-02 00:00:00",
            "Bob",
            "2024-01-01",
            None,
        )
        .await;

        let before = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule before handover");

        // Bob sells at the end of June; Bernice takes over on July 1 (tiled).
        let owners = queries::list_ownerships(&pool, &apt_b.id)
            .await
            .expect("list ownerships");
        let bob = owners.iter().find(|o| o.name == "Bob").expect("Bob");
        queries::update_ownership(
            &pool,
            &bob.id,
            "Bob",
            "bob@example.com",
            "2024-01-01",
            Some("2024-06-30"),
        )
        .await
        .expect("close Bob");
        add_owner(&pool, &apt_b.id, "Bernice", "2024-07-01", None).await;

        let after = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule after handover");

        // The duty week stays with the same apartment (ids of ownership
        // records map to their apartment).
        let ownerships = queries::list_ownerships_for_building(&pool, &building_id)
            .await
            .expect("list ownerships");
        let id_to_apartment: HashMap<&str, &str> = ownerships
            .iter()
            .map(|o| (o.id.as_str(), o.apartment_id.as_str()))
            .collect();
        for (wb, wa) in before.iter().zip(after.iter()) {
            let apt_before = wb
                .assignee_id
                .as_deref()
                .and_then(|id| id_to_apartment.get(id))
                .copied();
            let apt_after = wa
                .assignee_id
                .as_deref()
                .and_then(|id| id_to_apartment.get(id))
                .copied();
            assert_eq!(
                apt_before, apt_after,
                "week {} must keep its apartment",
                wb.iso_week
            );
        }

        // And the handover itself is visible: the first week after July 1 that
        // belongs to apartment B (2024-07-08, global index 235 = odd) is
        // covered by Bernice afterwards.
        let jul8 = NaiveDate::from_ymd_opt(2024, 7, 8).unwrap();
        let before_jul = before
            .iter()
            .find(|w| w.start == jul8)
            .expect("week 2024-07-08");
        assert_eq!(before_jul.assignee_name.as_deref(), Some("Bob"));
        let after_jul = after
            .iter()
            .find(|w| w.start == jul8)
            .expect("week 2024-07-08");
        assert_eq!(after_jul.assignee_name.as_deref(), Some("Bernice"));
    }

    /// The week containing a mid-week handover goes to the owner covering
    /// most of its days, so no week is ever unassigned.
    #[tokio::test]
    async fn transition_week_goes_to_majority_owner() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Alice",
            "2024-01-01",
            Some("2024-03-28"),
        )
        .await;
        // Bernice takes over the next day.
        add_owner(&pool, &apt.id, "Bernice", "2024-03-29", None).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        // Week Mar 25-31: Alice covers 4 days (Mon-Thu) vs Bernice 3 (Fri-Sun).
        let mar25 = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2024, 3, 25).unwrap())
            .expect("week 2024-03-25");
        assert_eq!(mar25.assignee_name.as_deref(), Some("Alice"));
        let apr1 = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2024, 4, 1).unwrap())
            .expect("week 2024-04-01");
        assert_eq!(apr1.assignee_name.as_deref(), Some("Bernice"));

        assert!(
            schedule.iter().all(|w| w.assignee_name.is_some()),
            "no week may be unassigned"
        );
    }

    /// An apartment whose first ownership starts mid-week joins the rotation
    /// with the first week that is at least half covered.
    #[tokio::test]
    async fn first_partially_covered_week_joins_later() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let _apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Alice",
            "2024-01-05",
            None,
        )
        .await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        let week1 = &schedule[0];
        assert_eq!(week1.start, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
        assert!(
            week1.assignee_name.is_none(),
            "week 1 is only 3 days covered"
        );
        let week2 = &schedule[1];
        assert_eq!(week2.assignee_name.as_deref(), Some("Alice"));
    }

    /// ISO week 1 of 2026 starts on 2025-12-29; an owner starting on
    /// 2026-01-01 covers 4 of its 7 days, so the New Year's week is assigned
    /// (previously this week was a hole for buildings whose owners start on
    /// January 1).
    #[tokio::test]
    async fn new_years_week_is_covered() {
        let (pool, building_id) = setup().await;
        let year = 2026;
        let _apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2026-01-01 00:00:00",
            "Alice",
            "2026-01-01",
            None,
        )
        .await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        let week1 = schedule.first().expect("week 1");
        assert_eq!(week1.start, NaiveDate::from_ymd_opt(2025, 12, 29).unwrap());
        assert_eq!(week1.assignee_name.as_deref(), Some("Alice"));
        assert!(schedule.iter().all(|w| w.assignee_name.is_some()));
    }

    /// A handover without a data gap (the next ownership starts the day after
    /// the previous one ends) leaves no unassigned week.
    #[tokio::test]
    async fn tiled_handover_leaves_no_gap() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "A",
            "2024-01-01",
            Some("2024-01-15"),
        )
        .await;

        // B takes over the very next day.
        add_owner(&pool, &apt.id, "B", "2024-01-16", None).await;

        let schedule = schedule_for_year(&building_id, year, &pool)
            .await
            .expect("schedule");

        // Every week has an assignee: no unassigned weeks.
        assert!(schedule.iter().all(|w| w.assignee_name.is_some()));

        // Week Jan 8-14 is fully A's; the transition week Jan 15-21 goes to B
        // (6 of 7 days); everything from Jan 22 on is B.
        let jan8 = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2024, 1, 8).unwrap())
            .expect("week 2024-01-08");
        assert_eq!(jan8.assignee_name.as_deref(), Some("A"));
        let jan15 = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2024, 1, 15).unwrap())
            .expect("week 2024-01-15");
        assert_eq!(jan15.assignee_name.as_deref(), Some("B"));
        let jan22 = schedule
            .iter()
            .find(|w| w.start == NaiveDate::from_ymd_opt(2024, 1, 22).unwrap())
            .expect("week 2024-01-22");
        assert_eq!(jan22.assignee_name.as_deref(), Some("B"));
    }

    #[tokio::test]
    async fn active_tenancy_delegates_owner_duty() {
        let (pool, building_id) = setup().await;
        let year = 2024;
        let apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Otto",
            "2024-01-01",
            None,
        )
        .await;
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
        let apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2024-01-01 00:00:00",
            "Otto",
            "2024-01-01",
            None,
        )
        .await;
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
        let apt = add_apartment(
            &pool,
            &building_id,
            "A",
            "2026-01-01 00:00:00",
            "Otto",
            "2026-01-01",
            None,
        )
        .await;
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
}
