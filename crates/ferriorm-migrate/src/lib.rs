#![warn(clippy::pedantic)]

//! Migration engine with automatic schema diffing.
//!
//! This crate compares two [`ferriorm_core::schema::Schema`] versions (the
//! "applied" state vs. the current `.ferriorm` file) and produces SQL migration
//! files. It supports two strategies for determining the applied state:
//!
//! - **Shadow database** ([`shadow`]) -- creates a temporary database, replays
//!   all existing migrations, and introspects the result. Accurate even when
//!   migration files have been manually edited.
//! - **Snapshot** ([`snapshot`]) -- uses a JSON schema snapshot stored alongside
//!   each migration. Fast and offline, but drifts if SQL files are edited.
//!
//! Key modules:
//!
//! - [`diff`] -- structural diff between two schemas, producing `MigrationStep`s.
//! - [`sql`] -- renders `MigrationStep`s into database-specific SQL.
//! - [`runner`] -- orchestrates creation, application, and status of migrations.
//! - [`introspect`] -- reads a live database and converts it to a `Schema` IR.
//! - [`state`] -- tracks applied migrations in the `_ferriorm_migrations` table.
//!
//! # Related crates
//!
//! - `ferriorm_core` -- the `Schema` IR this crate diffs against.
//! - `ferriorm_parser` -- produces the "current" `Schema` from the `.ferriorm` file.
//! - `ferriorm_cli` -- invokes this crate via `ferriorm migrate` commands.

pub mod diff;
pub mod introspect;
pub mod runner;
pub mod shadow;
pub mod snapshot;
pub mod sql;
pub mod state;

pub use diff::diff_schemas;
pub use runner::{MigrateError, MigrationRunner, MigrationStrategy};
