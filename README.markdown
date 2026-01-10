# Kehrkraft

Kehrkraft is a web application that generates downloadable PDF calendars showing who is responsible for Kehrwoche (stairwell cleaning) in a block of rented flats.

- Each plan represents one block of flats, has a name, and at least one administrator (contact).
- Each tenant has a name, email, tenancy start date, and an optional end date, stored in SQLite.
- A unique, somewhat secret URL serves a PDF for the current year (no login required for viewing the plan).

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

### Milestone 0: Bootstrap skeleton and deployable server

- Goal: Minimal Axum server, env-based config, health endpoint, Dockerized.
- Deliverables:
  - Config loader (PORT handling: bind to 0 if unset, log actual port).
  - GET /healthz -> 200 "ok".
  - Tracing/logging initialized.
  - Docker image builds and runs locally.
- Acceptance:
  - curl /healthz returns ok.
  - Container runs on plain HTTP; logs show bound port.

### Milestone 1: Database layer and migrations

- Goal: SQLite wired via SQLx, migrations applied on startup, in-memory DB supported.
- Deliverables:
  - migrations/0001_init.sql with tables: plans, plan_administrators, tenants; indexes on secret_slug and FKs.
  - DB pool creation and migrate-on-start.
  - Default DATABASE_URL to sqlite:kehrkraft.db.
  - Unit tests using sqlite::memory: with migrations.
- Acceptance:
  - App starts and applies migrations.
  - In-memory CRUD sanity tests pass.

### Milestone 2: Admin auth (Basic) and HTML master template

- Goal: Admin area behind Basic Auth; master template via PicoCSS.
- Deliverables:
  - Askama templates with base.html adapted from PicoCSS example (CDN usage).
  - Admin dashboard page (GET /admin).
  - Basic Auth protection using ADMIN_USER/ADMIN_PASS.
- Acceptance:
  - Visiting /admin prompts for Basic Auth and renders dashboard on success.

### Milestone 3: Plans CRUD (admin)

- Goal: Create/list/view/delete plans and plan administrators (contacts).
- Deliverables:
  - Secret slug auto-generated (128-bit random; base64url-no-pad or base32).
  - Queries: create_plan (with admins in a txn), list_plans, get_plan, delete_plan (cascade tenants/admins).
  - Admin pages for listing plans, creating new, viewing detail.
  - Unit tests covering plan creation/deletion.
- Acceptance:
  - Admin can create a plan, see its secret URL, and delete it.

### Milestone 4: Tenants CRUD and validations

- Goal: Manage tenants per plan with basic validation.
- Deliverables:
  - Queries: create/list/update/delete tenants.
  - Validation: start_date <= end_date (if provided), basic email format.
  - Admin UI under /admin/plans/{id}/tenants.
  - Unit tests for CRUD and validation.
- Acceptance:
  - Admin can add/edit/remove tenants; invalid inputs rejected.

### Milestone 5: Scheduling engine

- Goal: Compute weekly Kehrwoche assignments for the current year from active tenants.
- Approach:
  - ISO weeks (Mon–Sun). For each week, active tenants are those with start_date <= week_end AND (end_date IS NULL OR end_date >= week_start).
  - Deterministic order: sort by (start_date asc, name asc). Rotate by offset = (rotation_seed + week_index) % active_len.
  - If no active tenants, week is Unassigned.
- Deliverables:
  - scheduler module with schedule_for_year(plan_id, year, pool) -> Vec<WeekAssignment>.
  - Unit tests across year boundaries and gaps.
- Acceptance:
  - Deterministic assignments produced; gaps handled.

### Milestone 6: PDF generation with Typst and public URL

- Goal: Public secret URL returns a downloadable PDF for current year’s plan.
- Deliverables:
  - assets/typst/kehrwoche.typ template:
    - Inputs: plan_name, year, rows of (week, start, end, assignee_name, email).
    - Layout: title, table across pages, timestamp footer.
  - GET /p/{secret_slug}/kehrwoche.pdf:
    - Lookup plan by slug; compute schedule; generate a small .typ source that imports the template with injected data.
    - Run typst compile via subprocess; stream PDF with Content-Type application/pdf and Content-Disposition attachment (Kehrwoche-{plan}-{year}.pdf).
    - Temp dir per request; cleanup and timeouts.
  - Optional startup check for typst availability with a warning.
  - Integration/unit test asserting non-empty PDF body and headers using sqlite::memory:.
- Acceptance:
  - Downloading /p/{slug}/kehrwoche.pdf yields a valid PDF without authentication.

### Milestone 7: Admin polishing and HTML preview

- Goal: Admin preview of schedule with link to public PDF.
- Deliverables:
  - /admin/plans/{id}/schedule: Askama-rendered HTML of current year’s schedule.
  - UI polish within PicoCSS base template.
- Acceptance:
  - Admin can preview schedule and navigate to the public PDF.

### Milestone 8: End-to-end tests (browser-driven) with ephemeral in-memory DB

- Goal: Validate main flows via headless browser.
- Deliverables:
  - Test harness launches app bound to port 0; reads actual port; uses sqlite::memory: and test ADMIN_USER/PASS.
  - thirtyfour tests:
    - Authenticate to /admin (via Basic Auth header or URL credentials).
    - Create a plan and tenants.
    - Visit schedule preview.
    - Fetch PDF via HTTP client and assert 200 + application/pdf.
  - CI job installs chromedriver/geckodriver and runs E2E tests.
- Acceptance:
  - E2E passes end-to-end with isolated in-memory DB per test.

### Milestone 9: Packaging and deployment hardening

- Goal: Production-ready image and basic hardening.
- Deliverables:
- tower-http layers: Trace, Compression, basic security headers, body limits, simple rate limiting.
- Dockerfile: multi-stage build; typst installed in runtime; run as non-root.
- README updates for env vars, local dev, testing, and typst requirements.
- Acceptance:
  - Single docker run brings up the app with admin and PDF endpoints; logs are structured.

## Key implementation notes

- Basic Auth: Use axum-extra’s RequireAuthorizationLayer::basic(ADMIN_USER, ADMIN_PASS). Return 401 with WWW-Authenticate on failure.
- Port selection: If PORT unset, bind to 0 (OS assigns an ephemeral >1024 port); log actual port on startup.
- Secret slug entropy: 128-bit random, base64url-no-pad or Crockford base32; store in plans.secret_slug.
- Time/calendar: Use chrono ISO weeks; be consistent with timezone (UTC or local) and document choice; prefer local for tenant dates if relevant.
- Typst integration: Keep a reusable kehrwoche.typ; generate a minimal wrapper with serialized data to avoid code injection; per-request temp dir; cleanup.
- Testing DB: For sqlite::memory:, ensure at least one connection stays open for the pool lifetime so the DB persists within a test.
- Error handling: Map domain errors to 4xx/5xx; admin pages show friendly error templates.
- Internationalization: Keep strings ready for EN/DE; "Kehrwoche" as canonical term.

## License

Licensed under either of MIT or Apache-2.0, at your option.
