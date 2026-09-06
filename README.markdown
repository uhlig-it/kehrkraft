# Kehrkraft

# Overview

Kehrkraft is a web application for managing who is responsible for Kehrwoche (stairwell cleaning) in a building of (rented) apartments. A unique, somewhat secret URL serves a PDF for the current year, so that no login is required for viewing the plan.

# TODO

* iCal feed for a plan
* List people and their roles (admin, owner, tenant) and link to their objects
* Hourly database backup to S3 with retention (port sqlite-vault to sqlite-vault-rs)
* Internationalization: Keep strings ready for EN/DE; "Kehrwoche" as canonical term. Collect all strings that need translation and suggest German alternatives, so that we can support both languages
* Remove `rotation_seed` if really unused
* Support path variables:
  - `TYPST_BIN_PATH` (optional) - Full path to the typst executable that is to be used for generating the PDF invoice. Defaults to the first `typst` in the `$PATH`. If set and non-empty, that value is returned directly. Otherwise, "typst" is found in the system PATH.
  - `TYPST_SPOOL_DIR` (optional) - Path to an existing directory where the typst file for the PDF invoice, together with the JSON containing billing data and the Factur-X XML, will be stored. This contents of this directory are ephemeral, but they may be useful for troubleshooting PDF generation. Defaults to `$TMPDIR`.
* Reminders that duty is due for a tenant / owner
* When showing people `Bart Simpson <bart.simpson@example.com>`, omit the part in `<>`. Instead, show the email (as mailto: link) in their profile page
* Make buildings editable
* Switch to proper auth system
* Read-only JSON feed for hardware integrations
* Reminder eMails via forwardemail

# Domain Model

- A building has a name (max. 30 chars) and description (no limit) and at least one administrator (contact).
- A building consists of zero or more apartments
- An apartment has a name (max. 30 chars) and description (no limit). Conversely, an apartment belongs to a building.
- Each apartment has, at any point in time, an owner (we store name and email). Conversely, an owner might own apartments in zero or more buildings.
- Ownership of an apartment has a start date, and an optional end date.
- An apartment may be rented out to a tenant. For each tenant, we store a name and email address.
- Tenancy start date, and an optional end date.
- At any point in time, not more than one tenancy may be active for an apartment. It might happen that a tenancy ended and no new one exists (yet).
- At any point in time, not more than one ownership may be active for an apartment, and the ownership periods of an apartment tile its timeline seamlessly: the next ownership starts on the day after the previous one ends (enforced on create/update/delete). This guarantees that an apartment always has an owner and the schedule never has an unassigned week between two owners.
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

All tests use their own fresh in-memory SQLite database (`sqlite::memory:`); no setup needed.

```command
$ cargo test
```

This runs:

- Unit tests: DB `queries` and migrations, including the validation rules the database enforces via triggers (names, e-mail, date formats, ownership chain tiling, tenancy overlap, „every apartment has an owner“); scheduler.
- A PDF integration test that boots the real router, creates a building, and fetches `/p/{slug}/kehrwoche.pdf` over HTTP, asserting the body is a non-empty PDF.
- End-to-end tests (`tests/e2e/`) that drive the full app over HTTP: Basic Auth (401 without; buildings home page with), create building + apartment (with its first owner) + owner + tenancy (including rejection of overlapping tenancies and ownerships), drag reorder of apartments, schedule preview, and the public PDF fetch.

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

# Implementation

- Admin pages use a hand-rolled stylesheet (`/static/app.css`, embedded into the binary); no CSS framework.
- Admin authentication uses HTTP Basic Auth (credentials from environment variables).
- PDF rendering is done via Typst.
- The app listens on plain HTTP; port is read from env var PORT, otherwise binds to an OS-assigned ephemeral port (>1024).
- Testing includes unit tests, HTTP-level end-to-end tests, and a PDF integration test, each using a fresh in-memory SQLite database.

## Plan

Bottom-up, deployable after each step

- Language/runtime: Rust (Tokio)
- Web framework: Axum 0.7 + tower-http
- HTML templates: Askama (+ askama_axum), using a master template adapted from PicoCSS example
- Database: SQLite via SQLx (features: runtime-tokio, sqlite, chrono, uuid, migrate)
- Authentication: HTTP Basic Auth (admin) via axum-extra
- Config: environment variables (dotenvy for local dev)
- PDF rendering: Typst via CLI subprocess
- E2E testing: headless browser driven via thirtyfour + chromedriver (or geckodriver)
- Containerization: Docker multi-stage image including Typst in the runtime stage

Core domain:

- Building: id, name, description, secret_slug (URL token), rotation_seed (optional, integer), created_at, updated_at
- BuildingAdministrator: id, building_id, name, email, created_at
- Apartment: id, building_id, name, description, created_at
- Ownership: id, apartment_id, name, email, start_date, end_date NULL, created_at
- Tenancy: id, apartment_id, name, email, start_date, end_date NULL, created_at

Public secret URL:

- GET /p/{secret_slug}/kehrwoche.pdf returns the current year’s plan as a downloadable PDF

Environment variables (Kehrkraft's own variables are namespaced with the `KEHRKRAFT_` prefix; `RUST_LOG` is a tracing convention and stays unprefixed):

- KEHRKRAFT_PORT: listening port; if unset, reuse the port saved in `.kehrkraft-port` (or `KEHRKRAFT_PORT_FILE`) if still free, otherwise bind to 0 and persist the assigned port
- KEHRKRAFT_PORT_FILE: path of the file that persists the assigned dev port (default `.kehrkraft-port` in the working directory)
- KEHRKRAFT_DATABASE_URL: e.g., sqlite:kehrkraft.db; tests use sqlite::memory:
- KEHRKRAFT_ADMIN_USER, KEHRKRAFT_ADMIN_PASS: Basic Auth credentials for admin (not required when KEHRKRAFT_DEMO_MODE=true)
- KEHRKRAFT_DEMO_MODE: when true (or 1/yes/on), disables admin authentication and shows a "Demo Mode" banner on every page
- KEHRKRAFT_PUBLIC_URL: external base URL of the instance (e.g. https://kehrkraft.uhlig.it); printed as the QR code on the Kehrwoche PDF (`{PUBLIC_URL}/p/{slug}/kehrwoche.pdf`). The PDF is rendered without a QR code when unset
- RUST_LOG: optional logging level

Repository structure (evolves with milestones):

- Cargo.toml
- src/
  - main.rs
  - config.rs
  - web/
    - routes.rs
    - admin.rs
    - pdf.rs
    - templates/
      - base.html (custom-CSS master)
      - admin/*.html
  - db/
    - mod.rs
    - models.rs
    - queries.rs
    - migrate.rs
  - scheduler/
    - mod.rs
- migrations/
- assets/
  - typst/kehrwoche.typ
- tests/
  - unit/*.rs
  - e2e/*.rs
- Dockerfile
- .github/workflows/ci.yml
- .env.example
- README.markdown

## Milestones

> **Status:** M0–M5 ✅ done · M6 ✅ done · M7 ✅ done · M8 ✅ done · M9 ✅ done · M10 ✅ done. `cargo test` is green.

### Milestone 0: Bootstrap skeleton and deployable server — ✅ done

- Goal: Minimal Axum server, env-based config, health endpoint, Dockerized.
- Deliverables:
  - [x] Config loader (PORT handling: bind to 0 if unset, log actual port).
  - [x] GET /healthz -> 200 "ok".
  - [x] Tracing/logging initialized.
  - [x] Docker image builds and runs locally.
- Acceptance:
  - curl /healthz returns ok.
  - Container runs on plain HTTP; logs show bound port.

### Milestone 1: Database layer and migrations — ✅ done

- Goal: SQLite wired via SQLx, migrations applied on startup, in-memory DB supported.
- Deliverables:
  - [x] migrations/0001_init.sql with tables: plans, plan_administrators, tenants; indexes on secret_slug and FKs.
  - [x] DB pool creation and migrate-on-start.
  - [x] Default DATABASE_URL to sqlite:kehrkraft.db.
  - [x] Unit tests using sqlite::memory: with migrations.
- Acceptance:
  - App starts and applies migrations.
  - In-memory CRUD sanity tests pass.

### Milestone 2: Admin auth (Basic) and HTML master template — ✅ done

- Goal: Admin area behind Basic Auth; master template via PicoCSS.
- Deliverables:
  - [x] Askama templates with base.html adapted from PicoCSS example (CDN usage).
  - [x] Admin dashboard page (GET /admin).
  - [x] Basic Auth protection using ADMIN_USER/ADMIN_PASS.
- Note: Implemented as a hand-rolled middleware in main.rs (still returns 401 + WWW-Authenticate), not axum-extra's RequireAuthorizationLayer.
- Acceptance:
  - Visiting /admin prompts for Basic Auth and renders dashboard on success.

### Milestone 3: Plans CRUD (admin) — ✅ done

- Goal: Create/list/view/delete plans and plan administrators (contacts).
- Deliverables:
  - [x] Secret slug auto-generated (128-bit random; base64url-no-pad or base32).
  - [x] Queries: create_plan (with admins in a txn), list_plans, get_plan, delete_plan (cascade tenants/admins).
  - [x] Admin pages for listing plans, creating new, viewing detail.
  - [x] Unit tests covering plan creation/deletion.
- Acceptance:
  - Admin can create a plan, see its secret URL, and delete it.

### Milestone 4: Tenants CRUD and validations — ✅ done

- Goal: Manage tenants per plan with basic validation.
- Deliverables:
  - [x] Queries: create/list/update/delete tenants.
  - [x] Validation: start_date <= end_date (if provided), basic email format.
  - [x] Admin UI under /admin/plans/{id}/tenants.
  - [x] Unit tests for CRUD and validation.
- Acceptance:
  - Admin can add/edit/remove tenants; invalid inputs rejected.

### Milestone 5: Scheduling engine — ✅ done

- Goal: Compute weekly Kehrwoche assignments for the current year from active tenants.
- Approach:
  - ISO weeks (Mon–Sun). For each week, active tenants are those with start_date <= week_end AND (end_date IS NULL OR end_date >= week_start).
  - Deterministic order: sort by (start_date asc, name asc). Rotate by offset = (rotation_seed + week_index) % active_len.
  - If no active tenants, week is Unassigned.
- Deliverables:
  - [x] scheduler module with schedule_for_year(plan_id, year, pool) -> Vec<WeekAssignment>.
  - [x] Unit tests across year boundaries and gaps.
- Acceptance:
  - Deterministic assignments produced; gaps handled.

### Milestone 6: PDF generation with Typst and public URL — ✅ done

- Goal: Public secret URL returns a downloadable PDF for current year's plan.
- Deliverables:
  - [x] assets/typst/kehrwoche.typ template:
    - Inputs: plan_name, year, rows of (week, start, end, assignee_name, email).
    - Layout: title, table across pages, timestamp footer (footer rendered from the wrapper source in pdf.rs).
  - [x] GET /p/{secret_slug}/kehrwoche.pdf:
    - Lookup plan by slug; compute schedule; generate a small .typ source that imports the template with injected data.
    - Run typst compile via subprocess; stream PDF with Content-Type application/pdf and Content-Disposition attachment (Kehrwoche-{plan}-{year}.pdf).
    - Temp dir per request; cleanup and timeouts.
  - [x] Optional startup check for typst availability with a warning.
  - [x] Integration/unit test asserting non-empty PDF body and headers using sqlite::memory:.
- Note: Content-Disposition is now deliberately `inline` (not `attachment`); on Typst compile failure the temp dir is kept for inspection instead of being cleaned up.
- Acceptance:
  - Downloading /p/{slug}/kehrwoche.pdf yields a valid PDF without authentication.

### Milestone 7: Admin polishing and HTML preview — ✅ done

- Goal: Admin preview of schedule with link to public PDF.
- Deliverables:
  - [x] /admin/plans/{id}/schedule: Askama-rendered HTML of current year's schedule.
  - [x] UI polish within PicoCSS base template.
- Acceptance:
  - Admin can preview schedule and navigate to the public PDF.

### Milestone 8: End-to-end tests with ephemeral in-memory DB — ✅ done

- Goal: Validate main flows end-to-end.
- Deliverables:
  - [x] Test harness launches app bound to port 0; reads actual port; uses sqlite::memory: and test ADMIN_USER/PASS.
  - [x] E2E tests:
    - Authenticate to /admin (401 without credentials, dashboard with credentials).
    - Create a plan and tenants.
    - Visit schedule preview.
    - Fetch PDF via HTTP client and assert 200 + application/pdf.
  - [x] CI installs typst and runs the tests.
- Note: The original plan called for a headless browser (thirtyfour + chromedriver). That proved impractical: Chrome shows a native Basic Auth dialog that WebDriver cannot interact with, and the browser tests were slow and flaky. The same flows are instead exercised end-to-end at the HTTP layer against the real router; the UI can be verified manually in a browser.
- Acceptance:
  - E2E passes end-to-end with isolated in-memory DB per test.

### Milestone 9: Packaging and deployment hardening — ✅ done

- Goal: Production-ready image and basic hardening.
- Deliverables:
  - [x] tower-http layers: Trace, Compression, basic security headers, body limits, simple rate limiting.
  - [x] Dockerfile: multi-stage build.
  - [x] Dockerfile: typst installed in runtime stage.
  - [x] Dockerfile: run as non-root.
  - [x] README: env vars, local dev, testing, and typst requirements documented.
- Acceptance:
  - Single docker run brings up the app with admin and PDF endpoints; logs are structured.

### Milestone 10: Buildings, apartments, owners, and tenancies — ✅ done

- Goal: Align the implementation with the Domain Model: a building consists of apartments; each apartment has ownership records and optional tenancies; the scheduler assigns weeks round-robin to the apartments' owners and delegates to the active tenant where a tenancy overlaps the week.
- Decisions (confirmed during review):
  - Terminology: Haus = **building**, Wohnung = **apartment**; the managed entity was renamed from plan to building everywhere (tables, routes, templates, PDF filename) — “plan” now refers only to the derived yearly schedule and the public PDF.
  - The old `tenants` table was dropped in migration 0002 (its rows have no apartment association and cannot be migrated meaningfully; existing tenant data must be re-entered per apartment). The dev DB can be reset (`rm kehrkraft.db`).
  - The PDF lists the assignee only (tenant when delegated, otherwise owner); the HTML schedule preview marks delegated weeks with “(tenant)”.
- Approach:
  - Migration 0002 renames `plans` → `buildings` (+ `description` column), `plan_administrators` → `building_administrators`, creates `apartments`, `ownerships`, `tenancies`, and drops `tenants`.
  - Scheduling: per week, active owners are ownerships that contain the whole week (start_date <= week_start AND (end_date IS NULL OR end_date >= week_end)), sorted by (start_date asc, name asc); apartments without an active owner are skipped that week; week offset = (rotation_seed + week_index) % active_len (unchanged); if the chosen apartment has a tenancy containing that week, the assignee is the tenant, otherwise the owner.
  - Invariant: at most one active tenancy per apartment — overlapping tenancies are rejected on create/update with HTTP 400 (also covered end-to-end).
  - Invariant: at most one active ownership per apartment — overlapping ownerships are rejected on create/update with HTTP 400 (also covered end-to-end).
  - Validation: building and apartment names max. 30 chars; descriptions unlimited (domain model).
  - Admin UI: `/admin/buildings/{id}` shows description and apartment link; `/admin/buildings/{id}/apartments` lists apartments; the apartment page shows ownerships and tenancies with inline add forms plus edit/delete; the old tenant pages were removed.
- Deliverables:
  - [x] Migration 0002 (renames, apartments, ownerships, tenancies, plans.description; drop tenants).
  - [x] Models and queries: CRUD for buildings/apartments/ownerships/tenancies, tenancy overlap check.
  - [x] Scheduler rework: owners round-robin + tenant delegation.
  - [x] Admin templates: buildings, apartments, ownerships/tenancies edit pages.
  - [x] Tests: scheduler unit tests (round-robin, delegation, gaps), query tests, admin validation tests, updated e2e flows (create building → apartment → owner → tenancy → schedule preview → PDF; overlapping tenancy rejected).
- Acceptance:
  - Admin creates a building with apartments, records ownerships and tenancies.
  - Schedule preview and PDF assign weeks to owners round-robin; weeks with an active tenancy are delegated to the tenant.
  - Overlapping tenancies are rejected; invalid inputs rejected.
  - Public PDF still works without authentication.

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
