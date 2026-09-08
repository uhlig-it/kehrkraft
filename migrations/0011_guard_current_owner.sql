-- Deleting the period that covers the current date would leave the building
-- or apartment without a covering owner: the periods tile the timeline, so no
-- other period can take over. The existing delete guards only protect the sole
-- row (`*_guard_last`) and the interior of the chain (`*_guard_middle`), so a
-- current, open-ended period could be deleted whenever a closed historical
-- period remained — leaving a building or apartment without any owner today.
--
-- Both triggers carry the object-existence check so that cascading deletes
-- (building → building_owners, apartment → ownerships) pass unchanged, and the
-- coverage checks mirror `ownerships_guard_last`/`building_owners_guard_last`
-- so that cleanup of a legacy mixed state (0010) stays possible.

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
    SELECT RAISE(ABORT, 'Das Eigentum, das die Wohnung derzeit abdeckt, kann nicht gelöscht werden. Beenden Sie den Zeitraum stattdessen oder legen Sie zuerst einen neuen Eigentümer an.');
END;

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
    SELECT RAISE(ABORT, 'Der Gebäudeeigentümer, der das Gebäude derzeit abdeckt, kann nicht gelöscht werden, solange Wohnungen auf ihn angewiesen sind. Beenden Sie den Zeitraum stattdessen oder legen Sie zuerst einen Nachfolger an.');
END;