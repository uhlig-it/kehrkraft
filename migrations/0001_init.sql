-- Enforce foreign keys for this connection (the app also sets this at runtime)
PRAGMA foreign_keys = ON;

-- Plans
CREATE TABLE IF NOT EXISTS plans (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    secret_slug   TEXT NOT NULL UNIQUE,
    rotation_seed INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Plan administrators (contacts)
CREATE TABLE IF NOT EXISTS plan_administrators (
    id         TEXT PRIMARY KEY,
    plan_id    TEXT NOT NULL,
    name       TEXT NOT NULL,
    email      TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plan_administrators_plan_id ON plan_administrators(plan_id);

-- Tenants
CREATE TABLE IF NOT EXISTS tenants (
    id          TEXT PRIMARY KEY,
    plan_id     TEXT NOT NULL,
    name        TEXT NOT NULL,
    email       TEXT NOT NULL,
    start_date  TEXT NOT NULL,
    end_date    TEXT NULL,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tenants_plan_id ON tenants(plan_id);
