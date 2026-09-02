-- Milestone 10: Building/apartment domain model.
--
-- Renames `plans` -> `buildings` (the entity admins manage is now called
-- "building" everywhere), adds a description, and replaces the plan-level
-- `tenants` table with:
--   - apartments  (a building consists of apartments)
--   - ownerships  (apartment -> owner records, dated)
--   - tenancies   (apartment -> tenant records, dated; at most one active
--                  tenancy per apartment is enforced by the application)
--
-- NOTE: `tenants` is dropped. Its rows have no apartment association, so
-- they cannot be migrated meaningfully; any existing tenant data must be
-- re-entered per apartment.

ALTER TABLE plans RENAME TO buildings;
ALTER TABLE plan_administrators RENAME TO building_administrators;
ALTER TABLE building_administrators RENAME COLUMN plan_id TO building_id;

ALTER TABLE buildings ADD COLUMN description TEXT NOT NULL DEFAULT '';

DROP INDEX IF EXISTS idx_plan_administrators_plan_id;
CREATE INDEX IF NOT EXISTS idx_building_administrators_building_id
    ON building_administrators(building_id);

-- Apartments
CREATE TABLE IF NOT EXISTS apartments (
    id          TEXT PRIMARY KEY,
    building_id TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (building_id) REFERENCES buildings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_apartments_building_id ON apartments(building_id);

-- Ownership records: who owns an apartment, and for which period
CREATE TABLE IF NOT EXISTS ownerships (
    id            TEXT PRIMARY KEY,
    apartment_id  TEXT NOT NULL,
    name          TEXT NOT NULL,
    email         TEXT NOT NULL,
    start_date    TEXT NOT NULL,
    end_date      TEXT NULL,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (apartment_id) REFERENCES apartments(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ownerships_apartment_id ON ownerships(apartment_id);

-- Tenancy records: who rents an apartment, and for which period
CREATE TABLE IF NOT EXISTS tenancies (
    id            TEXT PRIMARY KEY,
    apartment_id  TEXT NOT NULL,
    name          TEXT NOT NULL,
    email         TEXT NOT NULL,
    start_date    TEXT NOT NULL,
    end_date      TEXT NULL,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (apartment_id) REFERENCES apartments(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tenancies_apartment_id ON tenancies(apartment_id);

DROP TABLE IF EXISTS tenants;