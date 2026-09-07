-- Building-level owners.
--
-- A building may be owned by a single entity (a person or, typically, a
-- housing company) that owns the whole building, so its apartments do not
-- need their own per-apartment ownership records. This mirrors the legal
-- reality of a block held by one owner (no Wohnungseigentümergemeinschaft):
-- the owner is recorded once for the building instead of being repeated for
-- every flat.
--
-- Periods tile the building's timeline exactly like `ownerships` tile an
-- apartment's timeline (no overlaps, no gaps, guard rails on delete).
--
-- The 0004 rules are adjusted accordingly:
--
--   * `apartments_require_ownership` now also accepts an apartment whose
--     building has a building owner covering the current date.
--   * `ownerships_guard_last` no longer blocks deleting the last per-apartment
--     ownership when a building owner covers the apartment (it stays owned).
--
-- The scheduler resolves each week's assignee as the apartment's own owner
-- when one covers the week and falls back to the building owner otherwise;
-- rented flats still delegate the Kehrwoche to their tenant.

CREATE TABLE IF NOT EXISTS building_owners (
    id            TEXT PRIMARY KEY,
    building_id   TEXT NOT NULL,
    name          TEXT NOT NULL,
    email         TEXT NOT NULL,
    start_date    TEXT NOT NULL,
    end_date      TEXT NULL,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (building_id) REFERENCES buildings(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX IF NOT EXISTS idx_building_owners_building_id
    ON building_owners(building_id);

-- Building owners: person/entity name, e-mail, dates (canonical YYYY-MM-DD).
CREATE TRIGGER building_owners_name_not_empty_insert BEFORE INSERT ON building_owners
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER building_owners_name_not_empty_update BEFORE UPDATE ON building_owners
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER building_owners_email_at_insert BEFORE INSERT ON building_owners
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER building_owners_email_at_update BEFORE UPDATE ON building_owners
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER building_owners_email_dot_insert BEFORE INSERT ON building_owners
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER building_owners_email_dot_update BEFORE UPDATE ON building_owners
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER building_owners_start_date_format_insert BEFORE INSERT ON building_owners
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Startdatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER building_owners_start_date_format_update BEFORE UPDATE ON building_owners
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Startdatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER building_owners_end_date_format_insert BEFORE INSERT ON building_owners
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Enddatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER building_owners_end_date_format_update BEFORE UPDATE ON building_owners
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Enddatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER building_owners_date_order_insert BEFORE INSERT ON building_owners
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Das Startdatum darf nicht nach dem Enddatum liegen.');
END;

CREATE TRIGGER building_owners_date_order_update BEFORE UPDATE ON building_owners
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Das Startdatum darf nicht nach dem Enddatum liegen.');
END;

-- The ownership periods of a building must tile its timeline: no overlaps,
-- each period starts on the day after its predecessor ends and ends on the
-- day before its successor starts. An open-ended period (NULL end) is only
-- possible as the last one, which the overlap check enforces.
CREATE TRIGGER building_owners_chain_overlap_insert BEFORE INSERT ON building_owners
WHEN EXISTS (
    SELECT 1 FROM building_owners o
    WHERE o.building_id = NEW.building_id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'Das Gebäudeeigentum überschneidet ein bestehendes Gebäudeeigentum');
END;

CREATE TRIGGER building_owners_chain_overlap_update BEFORE UPDATE ON building_owners
WHEN EXISTS (
    SELECT 1 FROM building_owners o
    WHERE o.building_id = NEW.building_id
      AND o.id != OLD.id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'Das Gebäudeeigentum überschneidet ein bestehendes Gebäudeeigentum');
END;

CREATE TRIGGER building_owners_chain_gap_prev_insert BEFORE INSERT ON building_owners
WHEN (SELECT MAX(o.end_date) FROM building_owners o
      WHERE o.building_id = NEW.building_id
        AND o.end_date IS NOT NULL
        AND o.end_date < NEW.start_date) IS NOT NULL
 AND date((SELECT MAX(o.end_date) FROM building_owners o
           WHERE o.building_id = NEW.building_id
             AND o.end_date IS NOT NULL
             AND o.end_date < NEW.start_date), '+1 day') != NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Das Gebäudeeigentum muss am Tag nach dem Ende des vorherigen Gebäudeeigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

CREATE TRIGGER building_owners_chain_gap_prev_update BEFORE UPDATE ON building_owners
WHEN (SELECT MAX(o.end_date) FROM building_owners o
      WHERE o.building_id = NEW.building_id
        AND o.id != OLD.id
        AND o.end_date IS NOT NULL
        AND o.end_date < NEW.start_date) IS NOT NULL
 AND date((SELECT MAX(o.end_date) FROM building_owners o
           WHERE o.building_id = NEW.building_id
             AND o.id != OLD.id
             AND o.end_date IS NOT NULL
             AND o.end_date < NEW.start_date), '+1 day') != NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Das Gebäudeeigentum muss am Tag nach dem Ende des vorherigen Gebäudeeigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

CREATE TRIGGER building_owners_chain_gap_next_insert BEFORE INSERT ON building_owners
WHEN (SELECT MIN(o.start_date) FROM building_owners o
      WHERE o.building_id = NEW.building_id
        AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')) IS NOT NULL
 AND date((SELECT MIN(o.start_date) FROM building_owners o
           WHERE o.building_id = NEW.building_id
             AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')), '-1 day') != COALESCE(NEW.end_date, '9999-12-31')
BEGIN
    SELECT RAISE(ABORT, 'Das Gebäudeeigentum muss am Tag vor dem Beginn des nächsten Gebäudeeigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

CREATE TRIGGER building_owners_chain_gap_next_update BEFORE UPDATE ON building_owners
WHEN (SELECT MIN(o.start_date) FROM building_owners o
      WHERE o.building_id = NEW.building_id
        AND o.id != OLD.id
        AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')) IS NOT NULL
 AND date((SELECT MIN(o.start_date) FROM building_owners o
           WHERE o.building_id = NEW.building_id
             AND o.id != OLD.id
             AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')), '-1 day') != COALESCE(NEW.end_date, '9999-12-31')
BEGIN
    SELECT RAISE(ABORT, 'Das Gebäudeeigentum muss am Tag vor dem Beginn des nächsten Gebäudeeigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

-- The last building owner can only be deleted while no apartment depends on
-- it, i.e. while every apartment still has a per-apartment ownership covering
-- the current date. The building-existence check lets the building's FK
-- cascade pass (the building row is already gone at that point, mirroring the
-- apartment-existence check of `ownerships_guard_last`).
CREATE TRIGGER building_owners_guard_last BEFORE DELETE ON building_owners
WHEN EXISTS (SELECT 1 FROM buildings WHERE id = OLD.building_id)
 AND (SELECT COUNT(*) FROM building_owners WHERE building_id = OLD.building_id) = 1
 AND EXISTS (
    SELECT 1 FROM apartments a
    WHERE a.building_id = OLD.building_id
      AND NOT EXISTS (
        SELECT 1 FROM ownerships o
        WHERE o.apartment_id = a.id
          AND o.start_date <= date('now', 'localtime')
          AND (o.end_date IS NULL OR o.end_date >= date('now', 'localtime'))
      )
 )
BEGIN
    SELECT RAISE(ABORT, 'Der letzte Gebäudeeigentümer kann nicht gelöscht werden, solange Wohnungen ohne eigenen Eigentümer auf ihn angewiesen sind.');
END;

-- Deleting a period between two others would open a hole in the building's
-- ownership chain; only the first or the last period may be deleted. The
-- building-existence check lets the building's FK cascade pass.
CREATE TRIGGER building_owners_guard_middle BEFORE DELETE ON building_owners
WHEN EXISTS (SELECT 1 FROM buildings WHERE id = OLD.building_id)
 AND EXISTS (
    SELECT 1 FROM building_owners o
    WHERE o.building_id = OLD.building_id
      AND o.id != OLD.id
      AND o.end_date IS NOT NULL
      AND o.end_date < OLD.start_date
 )
 AND EXISTS (
    SELECT 1 FROM building_owners o
    WHERE o.building_id = OLD.building_id
      AND o.id != OLD.id
      AND o.start_date > OLD.end_date
 )
BEGIN
    SELECT RAISE(ABORT, 'Dieses Gebäudeeigentum liegt zwischen zwei anderen Eigentümerzeiträumen. Es kann nur das erste oder das letzte Eigentum gelöscht werden.');
END;

-- An apartment can never exist without an owner: it either has a pending
-- ownership record in the current transaction (as before) or its building has
-- a building owner covering the current date.
DROP TRIGGER IF EXISTS apartments_require_ownership;
CREATE TRIGGER apartments_require_ownership BEFORE INSERT ON apartments
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE apartment_id = NEW.id)
 AND NOT EXISTS (
    SELECT 1 FROM building_owners bo
    WHERE bo.building_id = NEW.building_id
      AND bo.start_date <= date('now', 'localtime')
      AND (bo.end_date IS NULL OR bo.end_date >= date('now', 'localtime'))
 )
BEGIN
    SELECT RAISE(ABORT, 'Eine Wohnung muss mindestens ein Eigentum haben. Legen Sie die Wohnung zusammen mit ihrem ersten Eigentümer an, oder legen Sie zuerst einen Eigentümer für das gesamte Gebäude an.');
END;

-- The last per-apartment ownership of an existing apartment is normally
-- protected. With a covering building owner the apartment stays owned, so the
-- guard no longer applies.
DROP TRIGGER IF EXISTS ownerships_guard_last;
CREATE TRIGGER ownerships_guard_last BEFORE DELETE ON ownerships
WHEN EXISTS (SELECT 1 FROM apartments WHERE id = OLD.apartment_id)
 AND (SELECT COUNT(*) FROM ownerships WHERE apartment_id = OLD.apartment_id) <= 1
 AND NOT EXISTS (
    SELECT 1 FROM building_owners bo
    INNER JOIN apartments a ON a.building_id = bo.building_id
    WHERE a.id = OLD.apartment_id
      AND bo.start_date <= date('now', 'localtime')
      AND (bo.end_date IS NULL OR bo.end_date >= date('now', 'localtime'))
 )
BEGIN
    SELECT RAISE(ABORT, 'Das letzte Eigentum einer Wohnung kann nicht gelöscht werden, sonst hat die Wohnung keinen Eigentümer.');
END;
