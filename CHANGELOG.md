# Changelog

All notable changes to this project will be documented in this file.

<!-- git-cliff: end of header -->

## [0.1.0] - 2026-09-04

### Added

- Initial release: building, apartment, owner, and tenancy management, with at-most-one active ownership per apartment.
- Weekly rotation plans with CRUD and validations.
- Scheduling engine that assigns apartments/owners to weeks with conflict detection.
- Public PDF generation of the rotation schedule (Kehrwoche) via Typst, served from a secret per-building URL.
- Admin authentication via HTTP Basic Auth.
- SQLite persistence with embedded migrations; all tests run against fresh in-memory databases.
- Docker image bundling the app and the Typst CLI.
