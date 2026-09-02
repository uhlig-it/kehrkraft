//! End-to-end tests (Milestone 8).
//!
//! These exercise the full app over HTTP against the real router, database,
//! scheduler, and Typst pipeline. They are fast and hermetic; the user-facing
//! UI can be verified manually in a browser against a running instance.

mod flow;
mod harness;
