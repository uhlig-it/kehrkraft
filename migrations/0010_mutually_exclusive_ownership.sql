-- Ownership structure: a building is either wholly owned or a WEG — never both.
--
-- The ownership structure is chosen when creating the building (see the
-- "Eigentumsverhältnisse" choice on the new-building form): either one
-- person/company owns the whole building (building_owners), or the flats are
-- individually owned (ownerships). The two forms are mutually exclusive, and
-- the database now enforces that in both directions for every write:
--
--   * No new ownership record for an apartment whose building currently has
--     a building owner (covering the current date). Applying to both inserts
--     of the create-apartment flow (the ownership row is inserted before the
--     apartment, see the deferred FK) and the add-owner flow on an existing
--     apartment.
--   * No new building-owner period while any apartment of the building has
--     an ownership record (periods from the past count too: a building whose
--     flats were ever individually owned is a WEG).
--
-- Updates and deletes of existing rows are deliberately not restricted:
-- legacy rows that predate this migration (or that were entered before the
-- web UI guarded the flows) stay correctable and removable, so a mixed state
-- can always be cleaned up in either direction:
--
--   * deleting ownership rows is allowed while a covering building owner
--     exists (the relaxed `ownerships_guard_last` of 0005), and
--   * deleting a building owner is allowed while the apartments have their
--     own coverage (`building_owners_guard_last`).
--
-- Combined with those guard rails, the structure becomes immutable at the
-- database level: a wholly-owned building can neither gain apartment owners
-- (own ownership insert blocked) nor lose its owner while apartments depend
-- on it; a WEG can neither gain a building owner (blocked here) nor shed its
-- apartments' owners without a building owner covering them.

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
    SELECT RAISE(ABORT, 'Eine Wohnung eines Gebäudes mit Gebäudeeigentümer kann keinen eigenen Eigentümer haben.');
END;

-- The create-apartment flow inserts the first ownership before the apartment
-- (deferred FK), so at that point the apartment row – and with it the
-- building – does not exist yet. This trigger closes that gap at the moment
-- the apartment itself is inserted.
CREATE TRIGGER apartments_reject_ownership_when_building_owned_insert BEFORE INSERT ON apartments
WHEN EXISTS (SELECT 1 FROM ownerships WHERE apartment_id = NEW.id)
 AND EXISTS (
    SELECT 1 FROM building_owners bo
    WHERE bo.building_id = NEW.building_id
      AND bo.start_date <= date('now', 'localtime')
      AND (bo.end_date IS NULL OR bo.end_date >= date('now', 'localtime'))
 )
BEGIN
    SELECT RAISE(ABORT, 'Eine Wohnung eines Gebäudes mit Gebäudeeigentümer kann keinen eigenen Eigentümer haben.');
END;

CREATE TRIGGER building_owners_reject_when_apartments_owned_insert BEFORE INSERT ON building_owners
WHEN EXISTS (
    SELECT 1 FROM ownerships o
    INNER JOIN apartments a ON a.id = o.apartment_id
    WHERE a.building_id = NEW.building_id
)
BEGIN
    SELECT RAISE(ABORT, 'Ein Gebäude, dessen Wohnungen eigene Eigentümer haben, kann keinen Gebäudeeigentümer haben.');
END;