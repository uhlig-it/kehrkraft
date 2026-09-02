# Kehrkraft

Kehrkraft is a web application that generates downloadable PDF calendars showing who is responsible for Kehrwoche (stairwell cleaning) in a block of rented flats. A unique, somewhat secret URL serves a PDF for the current year, so that no login is required for viewing the plan.

# Domain Model

- A block has a name (max. 30 chars) and description (no limit) and at least one administrator (contact).
- A block consists of zero or more flats
- A flat has a name (max. 30 chars) and description (no limit). Conversly, a flat belongs to a block.
- Each flat has, at any point in time, an owner (we store name and email). Conversly, an owner might own zero or more flats.
- Ownership of a flat has a start date, and an optional end date.
- A flat may be rented out to a tenant. For each tenant, we store a name and email address.
- Tenancy start date, and an optional end date.
- At any point in time, not more than one tenacy may be active for a flat. It might happen that a tenacy ended and no new one exists (yet).
- A plan represents the responsibility for stairwell cleaning for a block during a year. The assignment is scheduled per the following rules:
  1. Responsibility is assigned to the owners of the flats round-robin.
  1. Responsibility lasts one week each. It starts Monday 00:00 and ends Sunday 23:59.
  1. If a tenacy is active for a week, the responsibility is delegated to the tenant.

# Develop

```command
$ PORT=3000 RUST_LOG=info ADMIN_USER=admin ADMIN_PASS=secret cargo watch -x "run"
```

Using docker:

```command
$ docker buildx build --tag kehrkraft:latest --load .
$ docker run --interactive --tty --rm --env PORT=3000 --env ADMIN_USER=admin --env ADMIN_PASS=secret --publish 3001:3000 kehrkraft
```

# Implementation

- Admin pages use a PicoCSS-based master template.
- Admin authentication uses HTTP Basic Auth (credentials from environment variables).
- PDF rendering is done via Typst.
- The app listens on plain HTTP; port is read from env var PORT, otherwise binds to an OS-assigned ephemeral port (>1024).
- Testing includes unit tests and end-to-end tests that drive a browser, each using a fresh in-memory SQLite database.

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

- Plan: id, name, secret_slug (URL token), rotation_seed (optional, integer), created_at, updated_at
- PlanAdministrator: id, plan_id, name, email, created_at
- Tenant: id, plan_id, name, email, start_date, end_date NULL, created_at

Public secret URL:

- GET /p/{secret_slug}/kehrwoche.pdf returns the current year’s plan as a downloadable PDF

Environment variables:

- PORT: listening port; if unset, bind to 0 and log the assigned port
- DATABASE_URL: e.g., sqlite:kehrkraft.db; tests use sqlite::memory:
- ADMIN_USER, ADMIN_PASS: Basic Auth credentials for admin
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
      - base.html (PicoCSS-based master)
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

> **Status:** M0–M5 ✅ done · M6 🟡 nearly done (PDF response test missing) · M7 ✅ done · M8 ❌ not started · M9 🟡 partial (tower-http hardening, Typst in Docker image, README polish pending). `cargo test` is green.

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

### Milestone 6: PDF generation with Typst and public URL — 🟡 4/5 done

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
  - [ ] Integration/unit test asserting non-empty PDF body and headers using sqlite::memory:.
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

### Milestone 8: End-to-end tests (browser-driven) with ephemeral in-memory DB — ❌ not started

- Goal: Validate main flows via headless browser.
- Deliverables:
  - [ ] Test harness launches app bound to port 0; reads actual port; uses sqlite::memory: and test ADMIN_USER/PASS.
  - [ ] thirtyfour tests:
    - Authenticate to /admin (via Basic Auth header or URL credentials).
    - Create a plan and tenants.
    - Visit schedule preview.
    - Fetch PDF via HTTP client and assert 200 + application/pdf.
  - [ ] CI job installs chromedriver/geckodriver and runs E2E tests.
- Acceptance:
  - E2E passes end-to-end with isolated in-memory DB per test.

### Milestone 9: Packaging and deployment hardening — 🟡 partial

- Goal: Production-ready image and basic hardening.
- Deliverables:
  - [ ] tower-http layers: Trace, Compression, basic security headers, body limits, simple rate limiting.
  - [x] Dockerfile: multi-stage build.
  - [ ] Dockerfile: typst installed in runtime stage (PDF generation currently fails inside the container).
  - [x] Dockerfile: run as non-root.
  - [x] README: env vars and local dev documented.
  - [ ] README: testing and typst requirements documented.
- Acceptance:
  - Single docker run brings up the app with admin and PDF endpoints; logs are structured.

## Key implementation notes

- Basic Auth: Currently a hand-rolled middleware in main.rs (401 + WWW-Authenticate on failure), functionally equivalent to axum-extra's RequireAuthorizationLayer::basic(ADMIN_USER, ADMIN_PASS).
- Port selection: If PORT unset, bind to 0 (OS assigns an ephemeral >1024 port); log actual port on startup.
- Secret slug entropy: 128-bit random, base64url-no-pad or Crockford base32; store in plans.secret_slug.
- Time/calendar: Use chrono ISO weeks; be consistent with timezone (UTC or local) and document choice; prefer local for tenant dates if relevant.
- Typst integration: Keep a reusable kehrwoche.typ; generate a minimal wrapper with serialized data to avoid code injection; per-request temp dir; cleanup.
- Testing DB: For sqlite::memory:, ensure at least one connection stays open for the pool lifetime so the DB persists within a test.
- Error handling: Map domain errors to 4xx/5xx; admin pages show friendly error templates.
- Internationalization: Keep strings ready for EN/DE; "Kehrwoche" as canonical term.

## License

Licensed under either of MIT or Apache-2.0, at your option.
