# Changelog

All notable changes to this project will be documented in this file.

<!-- git-cliff: end of header -->
## [0.6.0] - 2026-09-09

### Added

- Ask to end the previous ownership or tenancy on the day before when adding a new one
- Guard the current owner from deletion and warn before ending the chain
- Internationalize the UI with German/English and a language switcher
- Use Barlow in the Kehrwoche PDF like the web UI

### Changed

- Fix Renovate auto-merge for toolchain patches and action digest updates
- *(deps)* Update rust to v1.98.1 (#7)
- *(deps)* Update rust crate tokio to v1.53.1 (#9)
- *(deps)* Update softprops/action-gh-release digest to efb3536 (#2)
- Clean up changelog and drop the unreleased section

### Fixed

- *(deps)* Update rust crate tower-http to 0.7 (#12)
- *(deps)* Update rust crate base64 to 0.23 (#10)
- Make the release script resumable and refuse inconsistent release state

### Other

- Remove duplicates from changelog

## [0.5.1] - 2026-09-07

### Added

- Add iCal feed for the Kehrwoche schedule
- Polish resident links and PDF logo
- Expose rotation seed in the admin danger zone
- Make buildings editable including their Ansprechpartner
- Make the admin UI responsive on mobile
- Allow a single owner for an entire building
- Hourly encrypted S3 backups (port of sqlite-vault)
- Normalize owners, tenants and admins into people; enforce ownership structure

### Changed

- *(deps)* Update rust crate serde to v1.0.229 (#5)
- *(deps)* Update rust crate tracing-subscriber to v0.3.23 (#6)
- *(deps)* Update rust crate askama to v0.16.1 (#13)

### Fixed

- Accept apartment edit form without owner fields
- Fixup! Update TODOs
- Renumber backup canary migration to 0006

### Other

- Fix Renovate lookup for rustsec/audit-check pin
- Remove plan that was long implemented
- Fix release script

## [0.4.0] - 2026-09-06

### Added

- Add iCal feed for the Kehrwoche schedule
- Polish resident links and PDF logo
- Expose rotation seed in the admin danger zone

### Fixed

- Accept apartment edit form without owner fields

## [0.3.0] - 2026-09-06

### Added

- [**breaking**] Rework cleaning rotation for fairness and stability
- Redesign admin UI and localize it to German
- Balanced two-column Kehrwoche PDF with logo and QR code
- [**breaking**] Enforce validation in the database layer

### Changed

- Keep migration files byte-stable in pre-commit hooks
- Update TODOs
- Make tmuxinator paths relative to work from wtg worktrees
- Release 0.3.0

### Fixed

- Persist dev port across restarts and fall back when it is taken
- Fixup! chore: Update TODOs

## [0.2.0] - 2026-09-05

### Added

- Reorder apartments manually via drag handle
- Use hx-confirm for delete confirmations
- Add demo mode that disables auth and shows a banner

### Changed

- Add pre-commit hooks
- Add tmuxinator config
- Rework apartment drag reorder to the htmx Sortable.js pattern
- [**breaking**] Namespace Kehrkraft env vars with KEHRKRAFT_ prefix

### Other

- Render the actually running program version
- Release v0.2.0

## [0.1.0] - 2026-09-04

### Added

- Initial release: building, apartment, owner, and tenancy management, with at-most-one active ownership per apartment.
- Weekly rotation plans with CRUD and validations.
- Scheduling engine that assigns apartments/owners to weeks with conflict detection.
- Public PDF generation of the rotation schedule (Kehrwoche) via Typst, served from a secret per-building URL.
- Admin authentication via HTTP Basic Auth.
- SQLite persistence with embedded migrations; all tests run against fresh in-memory databases.
- Docker image bundling the app and the Typst CLI.
