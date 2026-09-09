# Kehrkraft

# Overview

Kehrkraft is a web application for managing who is responsible for Kehrwoche (stairwell cleaning) in a building of (rented) apartments. A unique, somewhat secret URL serves a PDF for the current year and an iCal feed for calendar subscriptions, so that no login is required for viewing the plan.

# TODO

* Use the same font in the PDF as in the web pages
* Switch to proper auth system
* Support path variables:
  - `TYPST_BIN_PATH` (optional) - Full path to the typst executable that is to be used for generating the PDF invoice. Defaults to the first `typst` in the `$PATH`. If set and non-empty, that value is returned directly. Otherwise, "typst" is searched in the system PATH.
  - `TYPST_SPOOL_DIR` (optional) - Path to an existing directory where the typst file for the PDF invoice, together with the assets we need for the page. This contents of this directory are ephemeral, but they may be useful for troubleshooting PDF generation. Defaults to `$TMPDIR`.
* Reminders that duty is due for a tenant / owner (via forwardemail)
* Read-only JSON feed for hardware integrations

# Domain Model

- A building has a name (max. 30 chars) and description (no limit) and an optional Ansprechpartner (administrator contact; set when creating or editing the building, may be omitted).
- A building consists of zero or more apartments
- An apartment has a name (max. 30 chars) and description (no limit). Conversely, an apartment belongs to a building.
- Owners are persons stored once (name and e-mail, migration 0007): each apartment has, at any point in time, an owner, and an owner may own a whole building, apartments in zero or more buildings, or both (e.g. a housing company with a WEG flat elsewhere). A person is identified by its e-mail address: entering a known e-mail reuses the person instead of creating a duplicate, and correcting name or e-mail updates every record of that person at once.
- Tenants and Ansprechpartner are persons as well (migration 0008): the same person row backs owner, tenant and administrator roles, so a person may be, say, an owner here and a tenant there. The people pages (`/admin/people`) list everyone with their roles and link to the objects they refer to.
- Ownership of an apartment has a start date, and an optional end date.
- A building may alternatively be owned by a single entity (one person/company owns all apartments, no Wohnungseigentümergemeinschaft); a building-owner period then covers the whole building and its apartments need no per-apartment ownership records. The ownership structure is chosen when creating the building; the two forms are mutually exclusive and enforced by the database (migration 0010: no ownership record for an apartment of a building with a covering building owner, and no building owner while apartments have owners) — so the structure is fixed once the building exists, and converting a wholly-owned building into individually owned flats (splitting it into a WEG) is not supported and out of scope for now.
- An apartment may be rented out to a tenant, who is taking over Kehrwoche during their tenancy.
- Tenancy start date, and an optional end date.
- At any point in time, not more than one tenancy may be active for an apartment. It might happen that a tenancy ended and no new one exists (yet).
- At any point in time, not more than one ownership may be active for an apartment, and the ownership periods of an apartment tile its timeline seamlessly: the next ownership starts on the day after the previous one ends (enforced on create/update/delete). This guarantees that an apartment always has an owner and the schedule never has an unassigned week between two owners. Building-owner periods tile the building's timeline the same way.
- A plan represents the responsibility for stairwell cleaning for a building during a year. The assignment is scheduled per the following rules:
  1. Responsibility is assigned round-robin over the apartments, in the order the apartments were created. The rotation counter continues across years, so the imbalance of years with 53 ISO weeks (one apartment serves one week more) rotates between the apartments over time instead of always hitting the same ones.
  1. Responsibility lasts one week each. It starts Monday 00:00 and ends Sunday 23:59.
  1. A week always has an assignee as long as at least one apartment has an owner: the week belongs to the owner whose period covers most of its days, so a transition week between two owners is resolved rather than left unassigned.
  1. If a tenancy covers a whole week, the responsibility is delegated to the tenant; otherwise the owner serves.

# Develop

The first start picks a free port and saves it to `.kehrkraft-port` in the working directory; later restarts (e.g. triggered by `cargo watch`) reuse that port. Delete the file or set `KEHRKRAFT_PORT` to pick a different port.

```command
$ RUST_LOG=info KEHRKRAFT_ADMIN_USER=admin KEHRKRAFT_ADMIN_PASS=secret cargo watch -x "run"
```

For a demo server without authentication (red "Demo Mode" banner shown on every page), set `KEHRKRAFT_DEMO_MODE=true`; `KEHRKRAFT_ADMIN_USER`/`KEHRKRAFT_ADMIN_PASS` are then not required:

```command
$ RUST_LOG=info KEHRKRAFT_DEMO_MODE=true cargo run
```

Using docker:

```command
$ docker buildx build --tag kehrkraft:latest --load .
$ docker run --interactive --tty --rm --env KEHRKRAFT_PORT=3000 --env KEHRKRAFT_ADMIN_USER=admin --env KEHRKRAFT_ADMIN_PASS=secret --publish 3001:3000 kehrkraft
```

The image bundles the Typst CLI, so PDF generation works inside the container out of the box.

# Testing

All tests use their own fresh SQLite database: in-memory for the app tests, and a temporary file for the backup tests (`VACUUM INTO` requires a file-backed source). No setup needed.

```command
$ cargo test
```

This runs:

- Unit tests: DB `queries` and migrations, including the validation rules the database enforces via triggers (names, e-mail, date formats, ownership chain tiling, tenancy overlap, „every apartment has an owner“); scheduler; backup (slot naming, age encryption round-trips, backup/verify against an in-memory object store).
- A PDF integration test that boots the real router, creates a building, and fetches `/p/{slug}/kehrwoche.pdf` over HTTP, asserting the body is a non-empty PDF.
- End-to-end tests (`tests/e2e/`) that drive the full app over HTTP: Basic Auth (401 without; buildings home page with), create building + apartment (with its first owner) + owner + tenancy (including rejection of overlapping tenancies and ownerships), drag reorder of apartments, schedule preview, and the public PDF/ iCal feed fetches.

Typst is required only for the two PDF tests; those skip automatically when the `typst` binary is missing. Install it via `brew install typst`, or see https://github.com/typst/typst for other platforms. The CI workflow installs Typst as well.

# Demo data

[`fixtures/demo.sql`](fixtures/demo.sql) seeds a demo building. The schema must exist before loading.

The app creates and migrates its database (`kehrkraft.db` in the working directory by default; override with `KEHRKRAFT_DATABASE_URL`) at startup, so start it first:

```command
$ KEHRKRAFT_PORT=3000 RUST_LOG=info KEHRKRAFT_ADMIN_USER=admin KEHRKRAFT_ADMIN_PASS=secret cargo run
```

In another terminal:

```command
$ sqlite3 kehrkraft.db < fixtures/demo.sql
```

# Backups (S3)

Kehrkraft takes hourly, encrypted backups of its SQLite database to any S3-compatible object store (AWS S3, Backblaze B2, MinIO). The concept is ported from [sqlite-vault](https://github.com/suhlig/sqlite-vault): a consistent snapshot via `VACUUM INTO`, age/scrypt encryption (restorable with `rage`), and deterministic object names that implement retention by overwriting — no deletion step, and at most ~85 objects in the bucket (24 hourly + 7 daily + 53 weekly + yearly). Hourly backups run for every hour except 04:00 UTC, which produces the daily/weekly/yearly backup instead.

Backups are opt-in: set `KEHRKRAFT_BACKUP_BUCKET` and the app starts a background task that takes a backup immediately at startup and then once per hour. A canary row inside the database prevents a restart within the same hour from overwriting the existing backup of that slot; `kehrkraft verify` uses the same row to check freshness.

**Make sure the S3 bucket has versioning disabled**; otherwise objects are never replaced and retention accumulates them forever.

## Configuration

| Variable | Required | Default | Description |
|---|---|---|---|
| `KEHRKRAFT_BACKUP_BUCKET` | yes* | – | S3 bucket name. Unset: backups disabled. Set: backups enabled. |
| `KEHRKRAFT_BACKUP_ACCESS_KEY` | yes | – | Access key / application key id. |
| `KEHRKRAFT_BACKUP_SECRET_KEY` | yes | – | Secret key / application key. |
| `KEHRKRAFT_BACKUP_PASSPHRASE` | yes | – | age passphrase; backups are never stored unencrypted. |
| `KEHRKRAFT_BACKUP_ENDPOINT` | no | AWS region endpoint | S3-compatible endpoint, e.g. `https://s3.us-west-004.backblazeb2.com` for B2 (`http://…` is allowed for local MinIO). |
| `KEHRKRAFT_BACKUP_REGION` | no | `us-east-1` | S3 region. |
| `KEHRKRAFT_BACKUP_PREFIX` | no | `kehrkraft` | Object name prefix. |
| `KEHRKRAFT_BACKUP_MAX_AGE_HOURS` | no | `26` | Maximum acceptable canary age for `kehrkraft verify`. |

All variables except the bucket are only required when backups are enabled.

Example for Backblaze B2:

1. Create the bucket and an application key with `listFiles, readFiles, writeFiles` permissions (see the [sqlite-vault README](https://github.com/suhlig/sqlite-vault) for exact commands).
1. Run the container:

   ```command
   $ docker run --rm \
       -e KEHRKRAFT_BACKUP_BUCKET=kehrkraft-backup \
       -e KEHRKRAFT_BACKUP_ENDPOINT=https://s3.us-west-004.backblazeb2.com \
       -e KEHRKRAFT_BACKUP_REGION=us-west-004 \
       -e KEHRKRAFT_BACKUP_ACCESS_KEY=... \
       -e KEHRKRAFT_BACKUP_SECRET_KEY=... \
       -e KEHRKRAFT_BACKUP_PASSPHRASE="horse battery staple" \
       kehrkraft
   ```

## Verification

`kehrkraft verify` downloads the latest hourly backup, decrypts it, runs `PRAGMA integrity_check`, and fails (exit non-zero) when the canary is older than `KEHRKRAFT_BACKUP_MAX_AGE_HOURS`. Run it from any host with the same `KEHRKRAFT_BACKUP_*` configuration, e.g. as a cron health check:

```command
$ KEHRKRAFT_BACKUP_BUCKET=kehrkraft-backup \
    KEHRKRAFT_BACKUP_ENDPOINT=https://s3.us-west-004.backblazeb2.com \
    KEHRKRAFT_BACKUP_REGION=us-west-004 \
    KEHRKRAFT_BACKUP_ACCESS_KEY=... \
    KEHRKRAFT_BACKUP_SECRET_KEY=... \
    KEHRKRAFT_BACKUP_PASSPHRASE="horse battery staple" \
    kehrkraft verify
```

## Restore

1. Download the `.age` file from the bucket, e.g. `b2 file download --no-progress b2://kehrkraft-backup/kehrkraft.hourly-09.db.age kehrkraft.db.age`.
1. Decrypt: `rage --decrypt --output kehrkraft.db kehrkraft.db.age`.
1. Check your data: `sqlite3 kehrkraft.db`.

# Implementation

- Admin pages use a hand-rolled stylesheet (`/static/app.css`, embedded into the binary); no CSS framework.
- Admin authentication uses HTTP Basic Auth (credentials from environment variables).
- PDF rendering is done via Typst.
- The iCal feed (`/p/{slug}/kehrwoche.ics`) is generated in Rust without extra dependencies (RFC 5545, all-day events).
- The app listens on plain HTTP; port is read from env var PORT, otherwise binds to an OS-assigned ephemeral port (>1024).
- Hourly encrypted backups (`src/backup/`) run in-process via a tokio task; object names, alias pointers, and the canary schema mirror sqlite-vault so backups stay interchangeable with that tooling.
- Testing includes unit tests, HTTP-level end-to-end tests, and a PDF integration test, each using a fresh in-memory SQLite database.

## Key implementation notes

- Basic Auth: Currently a hand-rolled middleware in main.rs (401 + WWW-Authenticate on failure), functionally equivalent to axum-extra's RequireAuthorizationLayer::basic(KEHRKRAFT_ADMIN_USER, KEHRKRAFT_ADMIN_PASS).
- Port selection: If KEHRKRAFT_PORT unset, reuse the port persisted in `.kehrkraft-port` so dev restarts (`cargo watch`) keep a stable port; fall back to binding port 0 (OS assigns an ephemeral >1024 port) and persist the assigned port. If KEHRKRAFT_PORT is set, the port file is not consulted.
- Secret slug entropy: 128-bit random, base64url-no-pad or Crockford base32; store in buildings.secret_slug.
- Time/calendar: Use chrono ISO weeks; be consistent with timezone (UTC or local) and document choice; prefer local for tenant dates if relevant.
- Typst integration: Keep a reusable kehrwoche.typ; generate a minimal wrapper with serialized data to avoid code injection; per-request temp dir; cleanup.
- Testing DB: For sqlite::memory:, ensure at least one connection stays open for the pool lifetime so the DB persists within a test.
- Error handling: Map domain errors to 4xx/5xx; admin pages show friendly error templates.

## License

Licensed under either of MIT or Apache-2.0, at your option.
