-- Validation triggers raise stable error codes instead of display text.
--
-- The triggers of 0004/0005/0007-0011 reject invalid data with
-- RAISE(ABORT, 'German sentence'), and the web layer showed that sentence
-- verbatim. From now on the database raises a stable `ERR_*` code and the web
-- layer translates the code into the UI language at render time (see
-- src/i18n.rs, keys `ERR_*`), so the database stays language-neutral.
--
-- Trigger names and bodies are unchanged; only the message literal differs.
-- The old trigger definitions are dropped and recreated below so existing
-- databases (whose migrations are checksum-locked) converge as well.
--
-- German sentences produced by codes (reference for the catalog):
--   ERR_NAME_EMPTY                    Name darf nicht leer sein.
--   ERR_NAME_TOO_LONG                 Name darf höchstens 30 Zeichen haben.
--   ERR_EMAIL_AT                      Die E-Mail-Adresse muss ein '@' enthalten.
--   ERR_EMAIL_DOT                     Die E-Mail-Adresse muss nach dem '@' einen Punkt enthalten.
--   ERR_START_DATE_FORMAT             Startdatum muss im Format JJJJ-MM-TT vorliegen.
--   ERR_END_DATE_FORMAT               Enddatum muss im Format JJJJ-MM-TT vorliegen.
--   ERR_START_AFTER_END               Das Startdatum darf nicht nach dem Enddatum liegen.
--   ERR_OWNERSHIP_OVERLAP             Das Eigentum überschneidet ein bestehendes Eigentum dieser Wohnung
--   ERR_TENANCY_OVERLAP               Das Mietverhältnis überschneidet ein bestehendes Mietverhältnis dieser Wohnung.
--   ERR_BUILDING_OWNER_OVERLAP        Das Gebäudeeigentum überschneidet ein bestehendes Gebäudeeigentum
--   ERR_OWNERSHIP_GAP_NEXT_START      Das Eigentum muss am Tag nach dem Ende des vorherigen Eigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.
--   ERR_BUILDING_OWNER_GAP_NEXT_START Das Gebäudeeigentum muss am Tag nach dem Ende des vorherigen Gebäudeeigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.
--   ERR_OWNERSHIP_GAP_PREV_END        Das Eigentum muss am Tag vor dem Beginn des nächsten Eigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.
--   ERR_BUILDING_OWNER_GAP_PREV_END   Das Gebäudeeigentum muss am Tag vor dem Beginn des nächsten Gebäudeeigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.
--   ERR_OWNERSHIP_LAST_DELETE         Das letzte Eigentum einer Wohnung kann nicht gelöscht werden, sonst hat die Wohnung keinen Eigentümer.
--   ERR_OWNERSHIP_MIDDLE_DELETE       Dieses Eigentum liegt zwischen zwei anderen Eigentümerzeiträumen. Es kann nur das erste oder das letzte Eigentum gelöscht werden.
--   ERR_BUILDING_OWNER_LAST_DELETE    Der letzte Gebäudeeigentümer kann nicht gelöscht werden, solange Wohnungen ohne eigenen Eigentümer auf ihn angewiesen sind.
--   ERR_BUILDING_OWNER_MIDDLE_DELETE  Dieses Gebäudeeigentum liegt zwischen zwei anderen Eigentümerzeiträumen. Es kann nur das erste oder das letzte Eigentum gelöscht werden.
--   ERR_APARTMENT_REQUIRES_OWNER      Eine Wohnung muss mindestens ein Eigentum haben. Legen Sie die Wohnung zusammen mit ihrem ersten Eigentümer an, oder legen Sie zuerst einen Eigentümer für das gesamte Gebäude an.
--   ERR_OWNERSHIP_REQUIRES_PERSON     Ein Eigentum muss einer Person zugeordnet sein.
--   ERR_BUILDING_OWNER_REQUIRES_PERSON Ein Gebäudeeigentum muss einer Person zugeordnet sein.
--   ERR_TENANCY_REQUIRES_PERSON       Ein Mietverhältnis muss einer Person zugeordnet sein.
--   ERR_ADMIN_REQUIRES_PERSON         Ein Ansprechpartner muss einer Person zugeordnet sein.
--   ERR_PERSON_EMAIL_TAKEN            Eine Person mit dieser E-Mail-Adresse existiert bereits.
--   ERR_OWNERSHIP_FORBIDDEN_WHEN_BUILDING_OWNER        Eine Wohnung eines Gebäudes mit Gebäudeeigentümer kann keinen eigenen Eigentümer haben.
--   ERR_BUILDING_OWNER_FORBIDDEN_WHEN_APARTMENT_OWNERS Ein Gebäude, dessen Wohnungen eigene Eigentümer haben, kann keinen Gebäudeeigentümer haben.
--   ERR_OWNERSHIP_CURRENT_DELETE      Das Eigentum, das die Wohnung derzeit abdeckt, kann nicht gelöscht werden. Beenden Sie den Zeitraum stattdessen oder legen Sie zuerst einen neuen Eigentümer an.
--   ERR_BUILDING_OWNER_CURRENT_DELETE Der Gebäudeeigentümer, der das Gebäude derzeit abdeckt, kann nicht gelöscht werden, solange Wohnungen auf ihn angewiesen sind. Beenden Sie den Zeitraum stattdessen oder legen Sie zuerst einen Nachfolger an.

-- ---------------------------------------------------------------------------
-- buildings: name rules (0004)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS buildings_name_not_empty_insert;
CREATE TRIGGER buildings_name_not_empty_insert BEFORE INSERT ON buildings
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_EMPTY');
END;

DROP TRIGGER IF EXISTS buildings_name_not_empty_update;
CREATE TRIGGER buildings_name_not_empty_update BEFORE UPDATE ON buildings
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_EMPTY');
END;

DROP TRIGGER IF EXISTS buildings_name_too_long_insert;
CREATE TRIGGER buildings_name_too_long_insert BEFORE INSERT ON buildings
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_TOO_LONG');
END;

DROP TRIGGER IF EXISTS buildings_name_too_long_update;
CREATE TRIGGER buildings_name_too_long_update BEFORE UPDATE ON buildings
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_TOO_LONG');
END;

-- ---------------------------------------------------------------------------
-- apartments: name rules (0004)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS apartments_name_not_empty_insert;
CREATE TRIGGER apartments_name_not_empty_insert BEFORE INSERT ON apartments
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_EMPTY');
END;

DROP TRIGGER IF EXISTS apartments_name_not_empty_update;
CREATE TRIGGER apartments_name_not_empty_update BEFORE UPDATE ON apartments
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_EMPTY');
END;

DROP TRIGGER IF EXISTS apartments_name_too_long_insert;
CREATE TRIGGER apartments_name_too_long_insert BEFORE INSERT ON apartments
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_TOO_LONG');
END;

DROP TRIGGER IF EXISTS apartments_name_too_long_update;
CREATE TRIGGER apartments_name_too_long_update BEFORE UPDATE ON apartments
WHEN length(NEW.name) > 30
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_TOO_LONG');
END;

-- An apartment can never exist without an ownership record (0004, final form
-- of 0005: a covering building owner also keeps the apartment owned).
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
    SELECT RAISE(ABORT, 'ERR_APARTMENT_REQUIRES_OWNER');
END;

-- ---------------------------------------------------------------------------
-- ownerships (0004 with the 0005/0007 adjustments)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS ownerships_start_date_format_insert;
CREATE TRIGGER ownerships_start_date_format_insert BEFORE INSERT ON ownerships
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS ownerships_start_date_format_update;
CREATE TRIGGER ownerships_start_date_format_update BEFORE UPDATE ON ownerships
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS ownerships_end_date_format_insert;
CREATE TRIGGER ownerships_end_date_format_insert BEFORE INSERT ON ownerships
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_END_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS ownerships_end_date_format_update;
CREATE TRIGGER ownerships_end_date_format_update BEFORE UPDATE ON ownerships
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_END_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS ownerships_date_order_insert;
CREATE TRIGGER ownerships_date_order_insert BEFORE INSERT ON ownerships
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_AFTER_END');
END;

DROP TRIGGER IF EXISTS ownerships_date_order_update;
CREATE TRIGGER ownerships_date_order_update BEFORE UPDATE ON ownerships
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_AFTER_END');
END;

DROP TRIGGER IF EXISTS ownerships_chain_overlap_insert;
CREATE TRIGGER ownerships_chain_overlap_insert BEFORE INSERT ON ownerships
WHEN EXISTS (
    SELECT 1 FROM ownerships o
    WHERE o.apartment_id = NEW.apartment_id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_OVERLAP');
END;

DROP TRIGGER IF EXISTS ownerships_chain_overlap_update;
CREATE TRIGGER ownerships_chain_overlap_update BEFORE UPDATE ON ownerships
WHEN EXISTS (
    SELECT 1 FROM ownerships o
    WHERE o.apartment_id = NEW.apartment_id
      AND o.id != OLD.id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_OVERLAP');
END;

DROP TRIGGER IF EXISTS ownerships_chain_gap_prev_insert;
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
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_GAP_NEXT_START');
END;

DROP TRIGGER IF EXISTS ownerships_chain_gap_prev_update;
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
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_GAP_NEXT_START');
END;

DROP TRIGGER IF EXISTS ownerships_chain_gap_next_insert;
CREATE TRIGGER ownerships_chain_gap_next_insert BEFORE INSERT ON ownerships
WHEN (SELECT MIN(o.start_date) FROM ownerships o
      WHERE o.apartment_id = NEW.apartment_id
        AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')) IS NOT NULL
 AND date((SELECT MIN(o.start_date) FROM ownerships o
           WHERE o.apartment_id = NEW.apartment_id
             AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')), '-1 day') != COALESCE(NEW.end_date, '9999-12-31')
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_GAP_PREV_END');
END;

DROP TRIGGER IF EXISTS ownerships_chain_gap_next_update;
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
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_GAP_PREV_END');
END;

-- The last per-apartment ownership of an existing apartment is protected
-- unless a covering building owner keeps the apartment owned (0005 form).
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
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_LAST_DELETE');
END;

DROP TRIGGER IF EXISTS ownerships_guard_middle;
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
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_MIDDLE_DELETE');
END;

DROP TRIGGER IF EXISTS ownerships_person_required_insert;
CREATE TRIGGER ownerships_person_required_insert BEFORE INSERT ON ownerships
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_REQUIRES_PERSON');
END;

DROP TRIGGER IF EXISTS ownerships_person_required_update;
CREATE TRIGGER ownerships_person_required_update BEFORE UPDATE ON ownerships
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_REQUIRES_PERSON');
END;

-- ---------------------------------------------------------------------------
-- tenancies (0004 with the 0008 person_id adjustment)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS tenancies_start_date_format_insert;
CREATE TRIGGER tenancies_start_date_format_insert BEFORE INSERT ON tenancies
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS tenancies_start_date_format_update;
CREATE TRIGGER tenancies_start_date_format_update BEFORE UPDATE ON tenancies
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS tenancies_end_date_format_insert;
CREATE TRIGGER tenancies_end_date_format_insert BEFORE INSERT ON tenancies
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_END_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS tenancies_end_date_format_update;
CREATE TRIGGER tenancies_end_date_format_update BEFORE UPDATE ON tenancies
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_END_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS tenancies_date_order_insert;
CREATE TRIGGER tenancies_date_order_insert BEFORE INSERT ON tenancies
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_AFTER_END');
END;

DROP TRIGGER IF EXISTS tenancies_date_order_update;
CREATE TRIGGER tenancies_date_order_update BEFORE UPDATE ON tenancies
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_AFTER_END');
END;

DROP TRIGGER IF EXISTS tenancies_no_overlap_insert;
CREATE TRIGGER tenancies_no_overlap_insert BEFORE INSERT ON tenancies
WHEN EXISTS (
    SELECT 1 FROM tenancies t
    WHERE t.apartment_id = NEW.apartment_id
      AND NEW.start_date <= COALESCE(t.end_date, '9999-12-31')
      AND t.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_TENANCY_OVERLAP');
END;

DROP TRIGGER IF EXISTS tenancies_no_overlap_update;
CREATE TRIGGER tenancies_no_overlap_update BEFORE UPDATE ON tenancies
WHEN EXISTS (
    SELECT 1 FROM tenancies t
    WHERE t.apartment_id = NEW.apartment_id
      AND t.id != OLD.id
      AND NEW.start_date <= COALESCE(t.end_date, '9999-12-31')
      AND t.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_TENANCY_OVERLAP');
END;

DROP TRIGGER IF EXISTS tenancies_person_required_insert;
CREATE TRIGGER tenancies_person_required_insert BEFORE INSERT ON tenancies
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_TENANCY_REQUIRES_PERSON');
END;

DROP TRIGGER IF EXISTS tenancies_person_required_update;
CREATE TRIGGER tenancies_person_required_update BEFORE UPDATE ON tenancies
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_TENANCY_REQUIRES_PERSON');
END;

-- ---------------------------------------------------------------------------
-- building_owners (0005 with the 0007 person_id adjustment)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS building_owners_start_date_format_insert;
CREATE TRIGGER building_owners_start_date_format_insert BEFORE INSERT ON building_owners
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS building_owners_start_date_format_update;
CREATE TRIGGER building_owners_start_date_format_update BEFORE UPDATE ON building_owners
WHEN strftime('%Y-%m-%d', NEW.start_date) IS NOT NEW.start_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS building_owners_end_date_format_insert;
CREATE TRIGGER building_owners_end_date_format_insert BEFORE INSERT ON building_owners
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_END_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS building_owners_end_date_format_update;
CREATE TRIGGER building_owners_end_date_format_update BEFORE UPDATE ON building_owners
WHEN NEW.end_date IS NOT NULL AND strftime('%Y-%m-%d', NEW.end_date) IS NOT NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_END_DATE_FORMAT');
END;

DROP TRIGGER IF EXISTS building_owners_date_order_insert;
CREATE TRIGGER building_owners_date_order_insert BEFORE INSERT ON building_owners
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_AFTER_END');
END;

DROP TRIGGER IF EXISTS building_owners_date_order_update;
CREATE TRIGGER building_owners_date_order_update BEFORE UPDATE ON building_owners
WHEN NEW.end_date IS NOT NULL AND NEW.start_date > NEW.end_date
BEGIN
    SELECT RAISE(ABORT, 'ERR_START_AFTER_END');
END;

DROP TRIGGER IF EXISTS building_owners_chain_overlap_insert;
CREATE TRIGGER building_owners_chain_overlap_insert BEFORE INSERT ON building_owners
WHEN EXISTS (
    SELECT 1 FROM building_owners o
    WHERE o.building_id = NEW.building_id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_OVERLAP');
END;

DROP TRIGGER IF EXISTS building_owners_chain_overlap_update;
CREATE TRIGGER building_owners_chain_overlap_update BEFORE UPDATE ON building_owners
WHEN EXISTS (
    SELECT 1 FROM building_owners o
    WHERE o.building_id = NEW.building_id
      AND o.id != OLD.id
      AND NEW.start_date <= COALESCE(o.end_date, '9999-12-31')
      AND o.start_date <= COALESCE(NEW.end_date, '9999-12-31')
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_OVERLAP');
END;

DROP TRIGGER IF EXISTS building_owners_chain_gap_prev_insert;
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
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_GAP_NEXT_START');
END;

DROP TRIGGER IF EXISTS building_owners_chain_gap_prev_update;
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
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_GAP_NEXT_START');
END;

DROP TRIGGER IF EXISTS building_owners_chain_gap_next_insert;
CREATE TRIGGER building_owners_chain_gap_next_insert BEFORE INSERT ON building_owners
WHEN (SELECT MIN(o.start_date) FROM building_owners o
      WHERE o.building_id = NEW.building_id
        AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')) IS NOT NULL
 AND date((SELECT MIN(o.start_date) FROM building_owners o
           WHERE o.building_id = NEW.building_id
             AND o.start_date > COALESCE(NEW.end_date, '9999-12-31')), '-1 day') != COALESCE(NEW.end_date, '9999-12-31')
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_GAP_PREV_END');
END;

DROP TRIGGER IF EXISTS building_owners_chain_gap_next_update;
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
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_GAP_PREV_END');
END;

DROP TRIGGER IF EXISTS building_owners_guard_last;
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
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_LAST_DELETE');
END;

DROP TRIGGER IF EXISTS building_owners_guard_middle;
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
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_MIDDLE_DELETE');
END;

DROP TRIGGER IF EXISTS building_owners_person_required_insert;
CREATE TRIGGER building_owners_person_required_insert BEFORE INSERT ON building_owners
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_REQUIRES_PERSON');
END;

DROP TRIGGER IF EXISTS building_owners_person_required_update;
CREATE TRIGGER building_owners_person_required_update BEFORE UPDATE ON building_owners
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_REQUIRES_PERSON');
END;

-- ---------------------------------------------------------------------------
-- people (0007/0009)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS people_name_not_empty_insert;
CREATE TRIGGER people_name_not_empty_insert BEFORE INSERT ON people
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_EMPTY');
END;

DROP TRIGGER IF EXISTS people_name_not_empty_update;
CREATE TRIGGER people_name_not_empty_update BEFORE UPDATE ON people
WHEN length(trim(NEW.name)) = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_NAME_EMPTY');
END;

DROP TRIGGER IF EXISTS people_email_at_insert;
CREATE TRIGGER people_email_at_insert BEFORE INSERT ON people
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_EMAIL_AT');
END;

DROP TRIGGER IF EXISTS people_email_at_update;
CREATE TRIGGER people_email_at_update BEFORE UPDATE ON people
WHEN instr(trim(NEW.email), '@') = 0
BEGIN
    SELECT RAISE(ABORT, 'ERR_EMAIL_AT');
END;

DROP TRIGGER IF EXISTS people_email_dot_insert;
CREATE TRIGGER people_email_dot_insert BEFORE INSERT ON people
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'ERR_EMAIL_DOT');
END;

DROP TRIGGER IF EXISTS people_email_dot_update;
CREATE TRIGGER people_email_dot_update BEFORE UPDATE ON people
WHEN substr(trim(NEW.email), instr(trim(NEW.email), '@') + 1) NOT LIKE '%.%'
BEGIN
    SELECT RAISE(ABORT, 'ERR_EMAIL_DOT');
END;

DROP TRIGGER IF EXISTS people_email_unique_insert;
CREATE TRIGGER people_email_unique_insert BEFORE INSERT ON people
WHEN EXISTS (
    SELECT 1 FROM people
    WHERE lower(trim(email)) = lower(trim(NEW.email))
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_PERSON_EMAIL_TAKEN');
END;

DROP TRIGGER IF EXISTS people_email_unique_update;
CREATE TRIGGER people_email_unique_update BEFORE UPDATE ON people
WHEN EXISTS (
    SELECT 1 FROM people
    WHERE lower(trim(email)) = lower(trim(NEW.email))
      AND id != NEW.id
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_PERSON_EMAIL_TAKEN');
END;

-- ---------------------------------------------------------------------------
-- building_administrators (0008)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS admins_person_required_insert;
CREATE TRIGGER admins_person_required_insert BEFORE INSERT ON building_administrators
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_ADMIN_REQUIRES_PERSON');
END;

DROP TRIGGER IF EXISTS admins_person_required_update;
CREATE TRIGGER admins_person_required_update BEFORE UPDATE ON building_administrators
WHEN NEW.person_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ERR_ADMIN_REQUIRES_PERSON');
END;

-- ---------------------------------------------------------------------------
-- mutually exclusive ownership structures (0010)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS ownerships_reject_when_building_owned_insert;
CREATE TRIGGER ownerships_reject_when_building_owned_insert BEFORE INSERT ON ownerships
WHEN EXISTS (
    SELECT 1 FROM apartments a
    WHERE a.id = NEW.apartment_id
      AND EXISTS (
          SELECT 1 FROM building_owners bo
          WHERE bo.building_id = a.building_id
            AND bo.start_date <= date('now', 'localtime')
            AND (bo.end_date IS NULL OR bo.end_date >= date('now', 'localtime'))
      )
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_FORBIDDEN_WHEN_BUILDING_OWNER');
END;

DROP TRIGGER IF EXISTS apartments_reject_ownership_when_building_owned_insert;
CREATE TRIGGER apartments_reject_ownership_when_building_owned_insert BEFORE INSERT ON apartments
WHEN EXISTS (SELECT 1 FROM ownerships WHERE apartment_id = NEW.id)
 AND EXISTS (
    SELECT 1 FROM building_owners bo
    WHERE bo.building_id = NEW.building_id
      AND bo.start_date <= date('now', 'localtime')
      AND (bo.end_date IS NULL OR bo.end_date >= date('now', 'localtime'))
 )
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_FORBIDDEN_WHEN_BUILDING_OWNER');
END;

DROP TRIGGER IF EXISTS building_owners_reject_when_apartments_owned_insert;
CREATE TRIGGER building_owners_reject_when_apartments_owned_insert BEFORE INSERT ON building_owners
WHEN EXISTS (
    SELECT 1 FROM ownerships o
    INNER JOIN apartments a ON a.id = o.apartment_id
    WHERE a.building_id = NEW.building_id
)
BEGIN
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_FORBIDDEN_WHEN_APARTMENT_OWNERS');
END;

-- ---------------------------------------------------------------------------
-- deleting the currently covering period (0011)
-- ---------------------------------------------------------------------------

DROP TRIGGER IF EXISTS ownerships_guard_current;
CREATE TRIGGER ownerships_guard_current BEFORE DELETE ON ownerships
WHEN EXISTS (SELECT 1 FROM apartments WHERE id = OLD.apartment_id)
 AND OLD.start_date <= date('now', 'localtime')
 AND (OLD.end_date IS NULL OR OLD.end_date >= date('now', 'localtime'))
 AND NOT EXISTS (
    SELECT 1 FROM building_owners bo
    INNER JOIN apartments a ON a.building_id = bo.building_id
    WHERE a.id = OLD.apartment_id
      AND bo.start_date <= date('now', 'localtime')
      AND (bo.end_date IS NULL OR bo.end_date >= date('now', 'localtime'))
 )
BEGIN
    SELECT RAISE(ABORT, 'ERR_OWNERSHIP_CURRENT_DELETE');
END;

DROP TRIGGER IF EXISTS building_owners_guard_current;
CREATE TRIGGER building_owners_guard_current BEFORE DELETE ON building_owners
WHEN EXISTS (SELECT 1 FROM buildings WHERE id = OLD.building_id)
 AND OLD.start_date <= date('now', 'localtime')
 AND (OLD.end_date IS NULL OR OLD.end_date >= date('now', 'localtime'))
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
    SELECT RAISE(ABORT, 'ERR_BUILDING_OWNER_CURRENT_DELETE');
END;
