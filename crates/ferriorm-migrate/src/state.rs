//! Tracks which migrations have been applied via the `_ferriorm_migrations` table.
//!
//! This module manages a metadata table in the user's database that records
//! each applied migration's name, SHA-256 checksum, and timestamp. It provides
//! functions to create the table, list applied migrations, mark new ones as
//! applied, and clear all records (for reset). Separate implementations exist
//! for `PostgreSQL` and `SQLite`.

use chrono::{DateTime, Utc};

#[cfg(feature = "postgres")]
use sqlx::PgPool;

#[cfg(feature = "sqlite")]
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AppliedMigration {
    pub id: i32,
    pub name: String,
    pub checksum: String,
    pub applied_at: DateTime<Utc>,
}

const MIGRATIONS_TABLE: &str = "_ferriorm_migrations";

// ─── PostgreSQL ───────────────────────────────────────────────────

/// Ensure the migrations tracking table exists (`PostgreSQL`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the DDL query fails.
#[cfg(feature = "postgres")]
pub async fn ensure_table(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS "{MIGRATIONS_TABLE}" (
            "id" SERIAL PRIMARY KEY,
            "name" TEXT NOT NULL UNIQUE,
            "checksum" TEXT NOT NULL,
            "applied_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#
    ))
    .execute(pool)
    .await?;
    Ok(())
}

/// Get all applied migrations, ordered by id (`PostgreSQL`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the query fails.
#[cfg(feature = "postgres")]
pub async fn get_applied(pool: &PgPool) -> Result<Vec<AppliedMigration>, sqlx::Error> {
    sqlx::query_as::<_, AppliedMigration>(&format!(
        r#"SELECT "id", "name", "checksum", "applied_at" FROM "{MIGRATIONS_TABLE}" ORDER BY "id""#
    ))
    .fetch_all(pool)
    .await
}

/// Record a migration as applied (`PostgreSQL`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the insert fails.
#[cfg(feature = "postgres")]
pub async fn mark_applied(pool: &PgPool, name: &str, checksum: &str) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(
        r#"INSERT INTO "{MIGRATIONS_TABLE}" ("name", "checksum") VALUES ($1, $2)"#
    ))
    .bind(name)
    .bind(checksum)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a migration record (for reset) (`PostgreSQL`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the delete fails.
#[cfg(feature = "postgres")]
pub async fn clear_all(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(r#"DELETE FROM "{MIGRATIONS_TABLE}""#))
        .execute(pool)
        .await?;
    Ok(())
}

// ─── SQLite ───────────────────────────────────────────────────────

/// SQLite-specific applied migration row.
///
/// `SQLite` stores `applied_at` as TEXT (ISO-8601), so we need a separate
/// `FromRow` type to read it as a string and then parse it.
#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, sqlx::FromRow)]
struct SqliteAppliedMigrationRow {
    id: i32,
    name: String,
    checksum: String,
    applied_at: String,
}

/// Ensure the migrations tracking table exists (`SQLite`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the DDL query fails.
#[cfg(feature = "sqlite")]
pub async fn ensure_table_sqlite(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS "{MIGRATIONS_TABLE}" (
            "id" INTEGER PRIMARY KEY AUTOINCREMENT,
            "name" TEXT NOT NULL UNIQUE,
            "checksum" TEXT NOT NULL,
            "applied_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )
        "#
    ))
    .execute(pool)
    .await?;
    Ok(())
}

/// Get all applied migrations, ordered by id (`SQLite`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the query fails.
#[cfg(feature = "sqlite")]
pub async fn get_applied_sqlite(pool: &SqlitePool) -> Result<Vec<AppliedMigration>, sqlx::Error> {
    let rows = sqlx::query_as::<_, SqliteAppliedMigrationRow>(&format!(
        r#"SELECT "id", "name", "checksum", "applied_at" FROM "{MIGRATIONS_TABLE}" ORDER BY "id""#
    ))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let applied_at =
                chrono::NaiveDateTime::parse_from_str(&row.applied_at, "%Y-%m-%dT%H:%M:%S%.fZ")
                    .unwrap_or_default()
                    .and_utc();

            AppliedMigration {
                id: row.id,
                name: row.name,
                checksum: row.checksum,
                applied_at,
            }
        })
        .collect())
}

/// Record a migration as applied (`SQLite`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the insert fails.
#[cfg(feature = "sqlite")]
pub async fn mark_applied_sqlite(
    pool: &SqlitePool,
    name: &str,
    checksum: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(
        r#"INSERT INTO "{MIGRATIONS_TABLE}" ("name", "checksum") VALUES ($1, $2)"#
    ))
    .bind(name)
    .bind(checksum)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove all migration records (for reset) (`SQLite`).
///
/// # Errors
///
/// Returns a [`sqlx::Error`] if the delete fails.
#[cfg(feature = "sqlite")]
pub async fn clear_all_sqlite(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(r#"DELETE FROM "{MIGRATIONS_TABLE}""#))
        .execute(pool)
        .await?;
    Ok(())
}
