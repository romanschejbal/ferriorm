//! SQL rendering for migration steps.
//!
//! Each database backend has its own [`SqlRenderer`] implementation that
//! converts a list of [`MigrationStep`]s into the appropriate DDL statements.
//! Use [`renderer_for`] to obtain the correct renderer for a given
//! `DatabaseProvider`.

pub mod postgres;
pub mod sqlite;

use crate::diff::MigrationStep;

/// Trait for rendering migration steps into SQL.
pub trait SqlRenderer {
    fn render(&self, steps: &[MigrationStep]) -> String;
}

/// Get the SQL renderer for the given provider.
#[must_use]
pub fn renderer_for(provider: ferriorm_core::types::DatabaseProvider) -> Box<dyn SqlRenderer> {
    match provider {
        ferriorm_core::types::DatabaseProvider::PostgreSQL
        | ferriorm_core::types::DatabaseProvider::MySQL => Box::new(postgres::PostgresRenderer), // TODO: MySQL renderer
        ferriorm_core::types::DatabaseProvider::SQLite => Box::new(sqlite::SqliteRenderer),
    }
}
