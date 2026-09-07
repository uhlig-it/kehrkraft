-- Tenants and Ansprechpartner join the people table.
--
-- Milestone 11 moved the owners (ownerships, building_owners) to `people`;
-- this migration completes the normalization for the two remaining embedded
-- contact records:
--
--   * `tenancies` gains `person_id` and drops its embedded name/e-mail.
--   * `building_administrators` (the Ansprechpartner) does the same.
--
-- Same rules as 0007: identity by e-mail (case-insensitive, trimmed), one
-- `people` row per distinct address across ALL four tables, period data
-- untouched, and people without any reference are removed automatically.
--
-- Ordering note: the extended cleanup triggers (below, at the end) reference
-- the `person_id` columns of ALL four tables, so they are only created after
-- every table has its column — SQLite validates trigger programs when the
-- schema changes, and a trigger referencing a not-yet-existing column would
-- break every later statement of this migration.

-- ---------------------------------------------------------------------------
-- tenancies: person_id instead of embedded name/e-mail
-- ---------------------------------------------------------------------------

-- Backfill people for tenant e-mails that are not a known person yet.
INSERT INTO people (id, name, email, created_at)
SELECT id, name, email, created_at
FROM (
    SELECT t.id, trim(t.name) AS name, trim(t.email) AS email, t.created_at,
           ROW_NUMBER() OVER (
               PARTITION BY lower(trim(t.email))
               ORDER BY t.created_at DESC, t.id DESC
           ) AS rn
    FROM tenancies t
    WHERE NOT EXISTS (
        SELECT 1 FROM people p
        WHERE lower(trim(p.email)) = lower(trim(t.email))
    )
)
WHERE rn = 1;

-- The per-row name/e-mail triggers move to `people`; drop them so the
-- columns can go.
DROP TRIGGER IF EXISTS tenancies_name_not_empty_insert;
DROP TRIGGER IF EXISTS tenancies_name_not_empty_update;
DROP TRIGGER IF EXISTS tenancies_email_at_insert;
DROP TRIGGER IF EXISTS tenancies_email_at_update;
DROP TRIGGER IF EXISTS tenancies_email_dot_insert;
DROP TRIGGER IF EXISTS tenancies_email_dot_update;

ALTER TABLE tenancies ADD COLUMN person_id TEXT REFERENCES people(id);

UPDATE tenancies
SET person_id = (
    SELECT p.id FROM people p
    WHERE lower(trim(p.email)) = lower(trim(tenancies.email))
);

ALTER TABLE tenancies DROP COLUMN name;
ALTER TABLE tenancies DROP COLUMN email;

CREATE INDEX IF NOT EXISTS idx_tenancies_person_id ON tenancies(person_id);

CREATE TRIGGER tenancies_person_required_insert BEFORE INSERT ON tenancies
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Mietverhältnis muss einer Person zugeordnet sein.');
END;

CREATE TRIGGER tenancies_person_required_update BEFORE UPDATE ON tenancies
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Mietverhältnis muss einer Person zugeordnet sein.');
END;

-- ---------------------------------------------------------------------------
-- building_administrators: person_id instead of embedded name/e-mail
-- ---------------------------------------------------------------------------

INSERT INTO people (id, name, email, created_at)
SELECT id, name, email, created_at
FROM (
    SELECT a.id, trim(a.name) AS name, trim(a.email) AS email, a.created_at,
           ROW_NUMBER() OVER (
               PARTITION BY lower(trim(a.email))
               ORDER BY a.created_at DESC, a.id DESC
           ) AS rn
    FROM building_administrators a
    WHERE NOT EXISTS (
        SELECT 1 FROM people p
        WHERE lower(trim(p.email)) = lower(trim(a.email))
    )
)
WHERE rn = 1;

DROP TRIGGER IF EXISTS admins_name_not_empty_insert;
DROP TRIGGER IF EXISTS admins_name_not_empty_update;
DROP TRIGGER IF EXISTS admins_email_at_insert;
DROP TRIGGER IF EXISTS admins_email_at_update;
DROP TRIGGER IF EXISTS admins_email_dot_insert;
DROP TRIGGER IF EXISTS admins_email_dot_update;

ALTER TABLE building_administrators ADD COLUMN person_id TEXT REFERENCES people(id);

UPDATE building_administrators
SET person_id = (
    SELECT p.id FROM people p
    WHERE lower(trim(p.email)) = lower(trim(building_administrators.email))
);

ALTER TABLE building_administrators DROP COLUMN name;
ALTER TABLE building_administrators DROP COLUMN email;

CREATE INDEX IF NOT EXISTS idx_building_administrators_person_id
    ON building_administrators(person_id);

CREATE TRIGGER admins_person_required_insert BEFORE INSERT ON building_administrators
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Ansprechpartner muss einer Person zugeordnet sein.');
END;

CREATE TRIGGER admins_person_required_update BEFORE UPDATE ON building_administrators
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Ansprechpartner muss einer Person zugeordnet sein.');
END;

-- ---------------------------------------------------------------------------
-- Cleanup: a person without any reference (in any of the four tables) is
-- removed when its last record goes. The 0007 triggers are recreated with
-- the two additional tables in their checks.
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS ownerships_people_cleanup;
CREATE TRIGGER ownerships_people_cleanup AFTER DELETE ON ownerships
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owners WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

DROP TRIGGER IF EXISTS building_owners_people_cleanup;
CREATE TRIGGER building_owners_people_cleanup AFTER DELETE ON building_owners
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owners WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

CREATE TRIGGER tenancies_people_cleanup AFTER DELETE ON tenancies
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owners WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

CREATE TRIGGER admins_people_cleanup AFTER DELETE ON building_administrators
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owners WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM tenancies WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_administrators WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;