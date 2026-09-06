-- Move validation into the database layer.
--
-- Previously the web layer enforced every rule in Rust and the database
-- accepted anything. From now on the database itself refuses invalid data
-- (no matter which client writes), by two mechanisms:
--
--   * Per-row rules (names, e-mail format, date format and ordering) and
--     cross-row rules (ownership chain tiling, tenancy overlap, apartments
--     always having an owner) are enforced by triggers that
--     RAISE(ABORT, 'message') with the same German messages the web UI
--     used to produce. The web layer no longer re-implements the rules; it
--     forwards the trigger message to the form.
--   * The ownerships FK becomes DEFERRABLE INITIALLY DEFERRED so the first
--     ownership of an apartment can be inserted before its apartment within
--     the creating transaction. The `apartments_require_ownership` trigger
--     then makes it impossible to insert an apartment without a pending
--     ownership row, and `ownerships_guard_last` makes it impossible to
--     delete the last ownership of an existing apartment.
--
-- Existing apartments that already have no ownership record are left
-- untouched (no fake ownership can be invented); the new rules guarantee
-- the invariant from now on.

-- Rebuild ownerships with a deferrable FK (SQLite cannot alter a FK).
CREATE TABLE ownerships_new (
    id            TEXT PRIMARY KEY,
    apartment_id  TEXT NOT NULL,
    name          TEXT NOT NULL,
    email         TEXT NOT NULL,
    start_date    TEXT NOT NULL,
    end_date      TEXT NULL,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (apartment_id) REFERENCES apartments(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED
);

INSERT INTO ownerships_new (id, apartment_id, name, email, start_date, end_date, created_at)
    SELECT id, apartment_id, name, email, start_date, end_date, created_at
    FROM ownerships;

DROP TABLE ownerships;
ALTER TABLE ownerships_new RENAME TO ownerships;

CREATE INDEX IF NOT EXISTS idx_ownerships_apartment_id ON ownerships(apartment_id);

-- Buildings: name must be non-empty and at most 30 characters.
CREATE TRIGGER buildings_name_not_empty_insert BEFORE INSERT ON buildings
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER buildings_name_not_empty_update BEFORE UPDATE ON buildings
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER buildings_name_too_long_insert BEFORE INSERT ON buildings
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'Name darf höchstens 30 Zeichen haben.');
END;

CREATE TRIGGER buildings_name_too_long_update BEFORE UPDATE ON buildings
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'Name darf höchstens 30 Zeichen haben.');
END;

-- Building administrators: contact name must be non-empty, e-mail well-formed.
CREATE TRIGGER admins_name_not_empty_insert BEFORE INSERT ON building_administrators
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name des Ansprechpartners darf nicht leer sein.');
END;

CREATE TRIGGER admins_name_not_empty_update BEFORE UPDATE ON building_administrators
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name des Ansprechpartners darf nicht leer sein.');
END;

CREATE TRIGGER admins_email_at_insert BEFORE INSERT ON building_administrators
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER admins_email_at_update BEFORE UPDATE ON building_administrators
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER admins_email_dot_insert BEFORE INSERT ON building_administrators
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER admins_email_dot_update BEFORE UPDATE ON building_administrators
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

-- Apartments: name must be non-empty and at most 30 characters.
CREATE TRIGGER apartments_name_not_empty_insert BEFORE INSERT ON apartments
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER apartments_name_not_empty_update BEFORE UPDATE ON apartments
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER apartments_name_too_long_insert BEFORE INSERT ON apartments
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'Name darf höchstens 30 Zeichen haben.');
END;

CREATE TRIGGER apartments_name_too_long_update BEFORE UPDATE ON apartments
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'Name darf höchstens 30 Zeichen haben.');
END;

-- An apartment can never exist without an ownership record: this trigger
-- requires an ownership row for the new apartment to be pending in the
-- current transaction (inserted before the apartment, see the deferred FK
-- above). Combined with `ownerships_guard_last`, every existing apartment
-- always has at least one owner.
CREATE TRIGGER apartments_require_ownership BEFORE INSERT ON apartments
WHEN NOT EXISTS (SELECT 1 FROM ownerships WHERE apartment_id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'Eine Wohnung muss mindestens ein Eigentum haben. Legen Sie die Wohnung zusammen mit ihrem ersten Eigentümer an.');
END;

-- Ownerships: person name, e-mail, dates (canonical YYYY-MM-DD, start <= end).
-- `strftime('%Y-%m-%d', x) IS NOT x` accepts exactly canonical calendar dates.
CREATE TRIGGER ownerships_name_not_empty_insert BEFORE INSERT ON ownerships
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER ownerships_name_not_empty_update BEFORE UPDATE ON ownerships
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER ownerships_email_at_insert BEFORE INSERT ON ownerships
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER ownerships_email_at_update BEFORE UPDATE ON ownerships
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER ownerships_email_dot_insert BEFORE INSERT ON ownerships
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER ownerships_email_dot_update BEFORE UPDATE ON ownerships
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER ownerships_start_date_format_insert BEFORE INSERT ON ownerships
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Startdatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER ownerships_start_date_format_update BEFORE UPDATE ON ownerships
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Startdatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER ownerships_end_date_format_insert BEFORE INSERT ON ownerships
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Enddatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER ownerships_end_date_format_update BEFORE UPDATE ON ownerships
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Enddatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER ownerships_date_order_insert BEFORE INSERT ON ownerships
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Das Startdatum darf nicht nach dem Enddatum liegen.');
END;

CREATE TRIGGER ownerships_date_order_update BEFORE UPDATE ON ownerships
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Das Startdatum darf nicht nach dem Enddatum liegen.');
END;

-- The ownership periods of an apartment must tile its timeline seamlessly:
-- no overlaps, each period starts on the day after its predecessor ends and
-- ends on the day before its successor starts. That guarantees the apartment
-- always has exactly one covering owner. An open-ended period (NULL end) is
-- only possible as the last one, which the overlap check enforces.
CREATE TRIGGER ownerships_chain_overlap_insert BEFORE INSERT ON ownerships
WHEN EXISTS (
    SELECT 1 FROM ownerships o
    WHERE o.apartment_id = NEW.apartment_id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'Das Eigentum überschneidet ein bestehendes Eigentum dieser Wohnung');
END;

CREATE TRIGGER ownerships_chain_overlap_update BEFORE UPDATE ON ownerships
WHEN EXISTS (
    SELECT 1 FROM ownerships o
    WHERE o.apartment_id = NEW.apartment_id
      AND o.id != OLD.id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'Das Eigentum überschneidet ein bestehendes Eigentum dieser Wohnung');
END;

CREATE TRIGGER ownerships_chain_gap_prev_insert BEFORE INSERT ON ownerships
WHEN (SELECT MAX(o.end_date) FROM ownerships o
      WHERE o.apartment_id = NEW.apartment_id
        AND o.end_date IS NOT NULL
        AND o.end_date < NEW.start_date) IS NOT NULL
 AND date((SELECT MAX(o.end_date) FROM ownerships o
           WHERE o.apartment_id = NEW.apartment_id
             AND o.end_date IS NOT NULL
             AND o.end_date < NEW.start_date), '+1 day') != NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Das Eigentum muss am Tag nach dem Ende des vorherigen Eigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

CREATE TRIGGER ownerships_chain_gap_prev_update BEFORE UPDATE ON ownerships
WHEN (SELECT MAX(o.end_date) FROM ownerships o
      WHERE o.apartment_id = NEW.apartment_id
        AND o.id != OLD.id
        AND o.end_date IS NOT NULL
        AND o.end_date < NEW.start_date) IS NOT NULL
 AND date((SELECT MAX(o.end_date) FROM ownerships o
           WHERE o.apartment_id = NEW.apartment_id
             AND o.id != OLD.id
             AND o.end_date IS NOT NULL
             AND o.end_date < NEW.start_date), '+1 day') != NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Das Eigentum muss am Tag nach dem Ende des vorherigen Eigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

CREATE TRIGGER ownerships_chain_gap_next_insert BEFORE INSERT ON ownerships
WHEN (SELECT MIN(o.start_date) FROM ownerships o
      WHERE o.apartment_id = NEW.apartment_id
        AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')) IS NOT NULL
 AND date((SELECT MIN(o.start_date) FROM ownerships o
           WHERE o.apartment_id = NEW.apartment_id
             AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')), '-1 day') != COALESCE(NEW.end_date, '9999-12-31')
BEGIN
    SELECT RAISE(ABORT, 'Das Eigentum muss am Tag vor dem Beginn des nächsten Eigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

CREATE TRIGGER ownerships_chain_gap_next_update BEFORE UPDATE ON ownerships
WHEN (SELECT MIN(o.start_date) FROM ownerships o
      WHERE o.apartment_id = NEW.apartment_id
        AND o.id != OLD.id
        AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')) IS NOT NULL
 AND date((SELECT MIN(o.start_date) FROM ownerships o
           WHERE o.apartment_id = NEW.apartment_id
             AND o.id != OLD.id
             AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')), '-1 day') != COALESCE(NEW.end_date, '9999-12-31')
BEGIN
    SELECT RAISE(ABORT, 'Das Eigentum muss am Tag vor dem Beginn des nächsten Eigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.');
END;

-- Deleting the last ownership of an apartment would leave the apartment
-- without an owner and is impossible. The apartment-existence check lets FK
-- cascades (deleting the apartment or building) pass: at that point the
-- apartment row is already gone.
CREATE TRIGGER ownerships_guard_last BEFORE DELETE ON ownerships
WHEN EXISTS (SELECT 1 FROM apartments WHERE id = OLD.apartment_id)
 AND (SELECT COUNT(*) FROM ownerships WHERE apartment_id = OLD.apartment_id) <= 1
BEGIN
    SELECT RAISE(ABORT, 'Das letzte Eigentum einer Wohnung kann nicht gelöscht werden, sonst hat die Wohnung keinen Eigentümer.');
END;

-- Deleting a period between two others would open a hole in the ownership
-- chain; only the first or the last period of the chain may be deleted.
CREATE TRIGGER ownerships_guard_middle BEFORE DELETE ON ownerships
WHEN EXISTS (SELECT 1 FROM apartments WHERE id = OLD.apartment_id)
 AND EXISTS (
    SELECT 1 FROM ownerships o
    WHERE o.apartment_id = OLD.apartment_id
      AND o.id != OLD.id
      AND o.end_date IS NOT NULL
      AND o.end_date < OLD.start_date
 )
 AND EXISTS (
    SELECT 1 FROM ownerships o
    WHERE o.apartment_id = OLD.apartment_id
      AND o.id != OLD.id
      AND o.start_date > OLD.end_date
 )
BEGIN
    SELECT RAISE(ABORT, 'Dieses Eigentum liegt zwischen zwei anderen Eigentümerzeiträumen. Es kann nur das erste oder das letzte Eigentum gelöscht werden.');
END;

-- Tenancies: person name, e-mail, dates (canonical YYYY-MM-DD, start <= end).
CREATE TRIGGER tenancies_name_not_empty_insert BEFORE INSERT ON tenancies
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER tenancies_name_not_empty_update BEFORE UPDATE ON tenancies
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'Name darf nicht leer sein.');
END;

CREATE TRIGGER tenancies_email_at_insert BEFORE INSERT ON tenancies
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER tenancies_email_at_update BEFORE UPDATE ON tenancies
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss ein ''@'' enthalten.');
END;

CREATE TRIGGER tenancies_email_dot_insert BEFORE INSERT ON tenancies
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER tenancies_email_dot_update BEFORE UPDATE ON tenancies
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'Die E-Mail-Adresse muss nach dem ''@'' einen Punkt enthalten.');
END;

CREATE TRIGGER tenancies_start_date_format_insert BEFORE INSERT ON tenancies
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Startdatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER tenancies_start_date_format_update BEFORE UPDATE ON tenancies
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'Startdatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER tenancies_end_date_format_insert BEFORE INSERT ON tenancies
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Enddatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER tenancies_end_date_format_update BEFORE UPDATE ON tenancies
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Enddatum muss im Format JJJJ-MM-TT vorliegen.');
END;

CREATE TRIGGER tenancies_date_order_insert BEFORE INSERT ON tenancies
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Das Startdatum darf nicht nach dem Enddatum liegen.');
END;

CREATE TRIGGER tenancies_date_order_update BEFORE UPDATE ON tenancies
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'Das Startdatum darf nicht nach dem Enddatum liegen.');
END;

-- At most one active tenancy per apartment at any point in time.
CREATE TRIGGER tenancies_no_overlap_insert BEFORE INSERT ON tenancies
WHEN EXISTS (
    SELECT 1 FROM tenancies t
    WHERE t.apartment_id = NEW.apartment_id
      AND NEW.start_date <= COALESCE(t.end_date, '9999-12-31')
      AND t.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'Das Mietverhältnis überschneidet ein bestehendes Mietverhältnis dieser Wohnung.');
END;

CREATE TRIGGER tenancies_no_overlap_update BEFORE UPDATE ON tenancies
WHEN EXISTS (
    SELECT 1 FROM tenancies t
    WHERE t.apartment_id = NEW.apartment_id
      AND t.id != OLD.id
      AND NEW.start_date <= COALESCE(t.end_date, '9999-12-31')
      AND t.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'Das Mietverhältnis überschneidet ein bestehendes Mietverhältnis dieser Wohnung.');
END;