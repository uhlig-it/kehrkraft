-- Manual ordering of apartments within a building.
--
-- Existing rows get position 0, so until reordered they keep the previous
-- deterministic (name-based) order via the secondary sort key. Newly created
-- apartments are appended at the end (position = max + 1).

ALTER TABLE apartments ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_apartments_building_position
    ON apartments(building_id, position);
