-- Owners become people (master data).
--
-- Previously every ownership period and building-owner period carried its own
-- copy of the owner's name and e-mail. A person owning a whole building AND an
-- apartment in a WEG elsewhere (or several apartments) therefore existed as
-- several unrelated rows, and correcting the contact data meant editing every
-- period by hand.
--
-- From now on owners are stored once in `people`, and the period tables
-- reference them:
--
--   * `ownerships` and `building_owners` gain `person_id` (NOT NULL,
--     enforced by triggers) and drop their `name`/`email` columns.
--   * The per-row name/e-mail validation triggers move from the period
--     tables to `people` (same German messages, so the web layer and its
--     tests keep working unchanged).
--   * The temporal invariants (chain tiling, guard rails, apartment
--     coverage) are untouched: they constrain periods, not persons.
--
-- Identity rules (documented, mirrored by the queries layer):
--
--   * A person is identified by its e-mail address, compared case-
--     insensitively after trimming; there is no unique constraint because
--     two humans can share a mailbox, but the application always reuses an
--     existing person with the same e-mail instead of creating a duplicate.
--   * `people.name`/`people.email` are current contact data shared by all
--     periods of the person; correcting them updates every period at once.
--
-- Backfill: one `people` row per distinct e-mail across both period tables
-- (the newest name wins, treating it as the most current contact data). All
-- existing rows then link to their person via the e-mail join.
--
-- People that no longer back any period are deleted automatically (cleanup
-- triggers); a person still referenced by a period cannot be deleted (FK).

CREATE TABLE IF NOT EXISTS people (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    email      TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- People: name must be non-empty, e-mail well-formed (as before on the
-- period tables, now enforced once per person).
CREATE TRIGGER people_name_not_empty_insert BEFORE INSERT ON people
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER people_name_not_empty_update BEFORE UPDATE ON people
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER people_email_at_insert BEFORE INSERT ON people
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER people_email_at_update BEFORE UPDATE ON people
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER people_email_dot_insert BEFORE INSERT ON people
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER people_email_dot_update BEFORE UPDATE ON people
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

-- Backfill one person per distinct e-mail (both period tables). The newest
-- record's name/creation time wins: current contact data, per the rules above.
INSERT INTO people (id, name, email, created_at)
SELECT id, name, email, created_at
FROM (
    SELECT o.id, trim(o.name) AS name, trim(o.email) AS email, o.created_at,
           ROW_NUMBER() OVER (
               PARTITION BY lower(trim(o.email))
               ORDER BY o.created_at DESC, o.id DESC
           ) AS rn
    FROM (
        SELECT id, name, email, created_at FROM ownerships
        UNION ALL
        SELECT id, name, email, created_at FROM building_owners
    ) o
)
WHERE rn = 1;

-- ---------------------------------------------------------------------------
-- ownerships: person_id instead of embedded name/e-mail
-- ---------------------------------------------------------------------------

-- The per-row name/e-mail triggers move to `people`; drop them so the
-- columns can go.
DROP TRIGGER IF EXISTS ownerships_name_not_empty_insert;
DROP TRIGGER IF EXISTS ownerships_name_not_empty_update;
DROP TRIGGER IF EXISTS ownerships_email_at_insert;
DROP TRIGGER IF EXISTS ownerships_email_at_update;
DROP TRIGGER IF EXISTS ownerships_email_dot_insert;
DROP TRIGGER IF EXISTS ownerships_email_dot_update;

ALTER TABLE ownerships ADD COLUMN person_id TEXT REFERENCES people(id);

UPDATE ownerships
SET person_id = (
    SELECT p.id FROM people p
    WHERE lower(trim(p.email)) = lower(trim(ownerships.email))
);

ALTER TABLE ownerships DROP COLUMN name;
ALTER TABLE ownerships DROP COLUMN email;

CREATE INDEX IF NOT EXISTS idx_ownerships_person_id ON ownerships(person_id);

-- Defensive invariant: every period belongs to a person. (SQLite cannot add
-- a NOT NULL column with a REFERENCES clause, so the rule is a trigger.)
CREATE TRIGGER ownerships_person_required_insert BEFORE INSERT ON ownerships
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Eigentum muss einer Person zugeordnet sein.');
END;

CREATE TRIGGER ownerships_person_required_update BEFORE UPDATE ON ownerships
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Eigentum muss einer Person zugeordnet sein.');
END;

-- A person without any period is meaningless; drop it when its last period
-- goes (delete or cascade). The checks run per deleted row, so a person
-- referenced by several periods is only removed with its very last one.
CREATE TRIGGER ownerships_people_cleanup AFTER DELETE ON ownerships
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owners WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;

-- ---------------------------------------------------------------------------
-- building_owners: person_id instead of embedded name/e-mail
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS building_owners_name_not_empty_insert;
DROP TRIGGER IF EXISTS building_owners_name_not_empty_update;
DROP TRIGGER IF EXISTS building_owners_email_at_insert;
DROP TRIGGER IF EXISTS building_owners_email_at_update;
DROP TRIGGER IF EXISTS building_owners_email_dot_insert;
DROP TRIGGER IF EXISTS building_owners_email_dot_update;

ALTER TABLE building_owners ADD COLUMN person_id TEXT REFERENCES people(id);

UPDATE building_owners
SET person_id = (
    SELECT p.id FROM people p
    WHERE lower(trim(p.email)) = lower(trim(building_owners.email))
);

ALTER TABLE building_owners DROP COLUMN name;
ALTER TABLE building_owners DROP COLUMN email;

CREATE INDEX IF NOT EXISTS idx_building_owners_person_id
    ON building_owners(person_id);

CREATE TRIGGER building_owners_person_required_insert BEFORE INSERT ON building_owners
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Gebäudeeigentum muss einer Person zugeordnet sein.');
END;

CREATE TRIGGER building_owners_person_required_update BEFORE UPDATE ON building_owners
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Ein Gebäudeeigentum muss einer Person zugeordnet sein.');
END;

CREATE TRIGGER building_owners_people_cleanup AFTER DELETE ON building_owners
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE person_id = OLD.person_id)
 AND NOT EXISTS (SELECT 1 FROM building_owners WHERE person_id = OLD.person_id)
BEGIN
    DELETE FROM people WHERE id = OLD.person_id;
END;
