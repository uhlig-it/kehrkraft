-- Multiple owners per ownership period, and a display name per person.
--
-- Previously each ownership period (apartment `ownerships` and building-level
-- `building_owners`) referenced exactly one person via `person_id`. A couple
-- owning a flat together therefore had to be recorded as a single made-up
-- person ("Anna & Ben Mustermann") and could not be addressed separately.
--
-- From now on a period references any number of persons through join tables:
--
--   * `ownership_people` and `building_owner_people` link one period to one
--     person; `position` is the display order within the period.
--   * `people` gains an optional `display_name`: when set, the UI shows it
--     instead of `name` for the person (owner, tenant, contact alike).
--
-- The period tables drop their `person_id` column; every other rule (chain
-- tiling, coverage, mutual exclusion of the two ownership forms) stays
-- untouched because it constrains periods, not persons.
--
-- Invariants enforced by the new triggers (mirroring the old ones):
--
--   * A period must have at least one person. The queries layer inserts the
--     join rows before the period row (the join FK is DEFERRABLE INITIALLY
--     DEFERRED), so a BEFORE INSERT trigger on the period table can require
--     a pending join row. Removing the last person of an existing period is
--     rejected by a guard on the join table (the period still exists then).
--   * People without any reference are deleted automatically (cleanup
--     triggers on the join tables, replacing the period-table triggers of
--     0007/0008; the tenancy/admin cleanup triggers are recreated with the
--     join tables in their checks).
--
-- Backfill: one join row per existing period row, linking its `person_id`.

-- ---------------------------------------------------------------------------
-- people: optional display name
-- ---------------------------------------------------------------------------

ALTER TABLE people ADD COLUMN display_name TEXT NULL;

-- ---------------------------------------------------------------------------
-- Join tables
-- ---------------------------------------------------------------------------

-- The FK to the period is DEFERRABLE INITIALLY DEFERRED so a join row may be
-- inserted before its (not yet existing) period row within the same
-- transaction; the person FK is immediate — the person always exists first.
CREATE TABLE IF NOT EXISTS ownership_people (
    ownership_id TEXT NOT NULL
        REFERENCES ownerships(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    person_id    TEXT NOT NULL REFERENCES people(id),
    position     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (ownership_id, person_id)
);

CREATE INDEX IF NOT EXISTS idx_ownership_people_person_id
    ON ownership_people(person_id);

CREATE TABLE IF NOT EXISTS building_owner_people (
    building_owner_id TEXT NOT NULL
        REFERENCES building_owners(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    person_id         TEXT NOT NULL REFERENCES people(id),
    position          INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (building_owner_id, person_id)
);

CREATE INDEX IF NOT EXISTS idx_building_owner_people_person_id
    ON building_owner_people(person_id);

-- Backfill one join row per period, keeping the period's person.
INSERT INTO ownership_people (ownership_id, person_id, position)
SELECT id, person_id, 0 FROM ownerships WHERE person_id IS NOT NULL;

INSERT INTO building_owner_people (building_owner_id, person_id, position)
SELECT id, person_id, 0 FROM building_owners WHERE person_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- ownerships / building_owners: person_id moves to the join tables
-- ---------------------------------------------------------------------------

-- The per-period person-required triggers and all four person cleanup
-- triggers reference the dropped columns (a cleanup trigger's WHEN clause
-- looks at `person_id` of every period table), so they must all be dropped
-- before the first DROP COLUMN.
DROP TRIGGER IF EXISTS ownerships_person_required_insert;
DROP TRIGGER IF EXISTS ownerships_person_required_update;
DROP TRIGGER IF EXISTS building_owners_person_required_insert;
DROP TRIGGER IF EXISTS building_owners_person_required_update;
DROP TRIGGER IF EXISTS ownerships_people_cleanup;
DROP TRIGGER IF EXISTS building_owners_people_cleanup;
DROP TRIGGER IF EXISTS tenancies_people_cleanup;
DROP TRIGGER IF EXISTS admins_people_cleanup;

DROP INDEX IF EXISTS idx_ownerships_person_id;
ALTER TABLE ownerships DROP COLUMN person_id;

DROP INDEX IF EXISTS idx_building_owners_person_id;
ALTER TABLE building_owners DROP COLUMN person_id;

-- A new period must come with at least one person: the queries layer inserts
-- the join rows first (deferred FK), so the trigger finds them here.
CREATE TRIGGER ownerships_people_required_insert BEFORE INSERT ON ownerships
WHEN NOT EXISTS (SELECT 1 FROM ownership_people WHERE ownership_id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_REQUIRES_PERSON');
END;

CREATE TRIGGER building_owners_people_required_insert BEFORE INSERT ON building_owners
WHEN NOT EXISTS (SELECT 1 FROM building_owner_people WHERE building_owner_id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_REQUIRES_PERSON');
END;

-- The last person of an existing period cannot be removed. When the period
-- itself is deleted (directly or via cascade), its rows are gone before the
-- trigger runs, so the period-existence check lets those deletes pass.
CREATE TRIGGER ownership_people_guard_last AFTER DELETE ON ownership_people
WHEN EXISTS (SELECT 1 FROM ownerships WHERE id = OLD.ownership_id)
 AND NOT EXISTS (SELECT 1 FROM ownership_people WHERE ownership_id = OLD.ownership_id)
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_REQUIRES_PERSON');
END;

CREATE TRIGGER building_owner_people_guard_last AFTER DELETE ON building_owner_people
WHEN EXISTS (SELECT 1 FROM building_owners WHERE id = OLD.building_owner_id)
 AND NOT EXISTS (SELECT 1 FROM building_owner_people WHERE building_owner_id = OLD.building_owner_id)
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_REQUIRES_PERSON');
END;

-- Cleanup: a person without any reference (in any of the four role tables) is
-- removed when its last reference goes, exactly like the 0007/0008 triggers —
-- now watching the join rows, which are the last rows to go (they fire for
-- explicit deletions and for the period's cascade alike).
CREATE TRIGGER ownership_people_cleanup AFTER DELETE ON ownership_people
WHEN NOT EXISTS (SELECT 1 FROM ownership_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owner_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

CREATE TRIGGER building_owner_people_cleanup AFTER DELETE ON building_owner_people
WHEN NOT EXISTS (SELECT 1 FROM ownership_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owner_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

-- ---------------------------------------------------------------------------
-- tenancies / building_administrators: cleanup checks now span the join tables
-- ---------------------------------------------------------------------------

CREATE TRIGGER tenancies_people_cleanup AFTER DELETE ON tenancies
WHEN NOT EXISTS (SELECT 1 FROM ownership_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owner_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

CREATE TRIGGER admins_people_cleanup AFTER DELETE ON building_administrators
WHEN NOT EXISTS (SELECT 1 FROM ownership_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owner_people WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;