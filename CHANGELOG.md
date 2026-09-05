# Changelog

All notable changes to this project will be documented in this file.

<!-- git-cliff: end of header -->
## [Unreleased]

### Changed

- [**breaking**] The rotation is now anchored to apartments (in the order they were created) instead of ownership records, with a continuous counter across years, so that owner or tenant changes mid-year no longer shift the duty weeks of other apartments, and the +1 imbalance of years with 53 ISO weeks rotates between the apartments instead of always hitting the same ones.
- [**breaking**] Ownership periods of an apartment must now tile its timeline seamlessly (the next ownership starts on the day after the previous one ends; enforced on create/update/delete). Transition weeks between two ownerships are assigned to the owner covering most of the week, so there are no more unassigned weeks.
- The apartment order in the admin list is now explicitly display-only (note added); it never influences scheduling.

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

## [0.2.0] - 2026-09-05

### Added

- Reorder apartments manually via drag handle
- Use hx-confirm for delete confirmations
- Add demo mode that disables auth and shows a banner

### Changed

- Rework apartment drag reorder to the htmx Sortable.js pattern
- [**breaking**] Namespace Kehrkraft env vars with KEHRKRAFT_ prefix

### Other

- Render the actually running program version
- Add pre-commit hooks
- Add tmuxinator config

## [0.1.0] - 2026-09-04

### Added

- Initial release: building, apartment, owner, and tenancy management, with at-most-one active ownership per apartment.
- Weekly rotation plans with CRUD and validations.
- Scheduling engine that assigns apartments/owners to weeks with conflict detection.
- Public PDF generation of the rotation schedule (Kehrwoche) via Typst, served from a secret per-building URL.
- Admin authentication via HTTP Basic Auth.
- SQLite persistence with embedded migrations; all tests run against fresh in-memory databases.
- Docker image bundling the app and the Typst CLI.
