//! Shadow database strategy for migration diffing.
//!
//! 1. Create a temporary database
//! 2. Apply all existing migration SQL files to it
//! 3. Introspect the resulting schema
//! 4. Diff against the current .ferriorm schema
//! 5. Drop the temporary database
//!
//! This is the default strategy because it respects manual edits
//! to migration.sql files.

#[cfg(feature = "postgres")]
use sqlx::PgPool;
use std::path::Path;

use crate::introspect;

// ─── PostgreSQL shadow database ───────────────────────────────────

/// Run the shadow database diffing process and return the introspected schema.
///
/// `main_url` is the connection URL for the main database (used to connect
/// to `PostgreSQL` to create/drop the shadow database).
///
/// # Errors
///
/// Returns a [`ShadowError`] if any step of the shadow database process fails.
#[cfg(feature = "postgres")]
pub async fn introspect_via_shadow(
    main_url: &str,
    migrations_dir: &Path,
) -> Result<ferriorm_core::schema::Schema, ShadowError> {
    let shadow_db_name = format!("_ferriorm_shadow_{}", uuid::Uuid::new_v4().simple());

    // Connect to the main PostgreSQL server (without database name)
    let server_url = strip_database_from_url(main_url);
    let server_pool = PgPool::connect(&server_url)
        .await
        .map_err(|e| ShadowError::Connection(format!("Failed to connect to server: {e}")))?;

    // Create shadow database
    sqlx::query(&format!(r#"CREATE DATABASE "{shadow_db_name}""#))
        .execute(&server_pool)
        .await
        .map_err(|e| ShadowError::Create(format!("Failed to create shadow database: {e}")))?;

    // Connect to the shadow database
    let shadow_url = replace_database_in_url(main_url, &shadow_db_name);
    let shadow_pool = match PgPool::connect(&shadow_url).await {
        Ok(pool) => pool,
        Err(e) => {
            // Clean up on failure
            let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{shadow_db_name}""#))
                .execute(&server_pool)
                .await;
            return Err(ShadowError::Connection(format!(
                "Failed to connect to shadow database: {e}"
            )));
        }
    };

    // Apply all existing migrations in order
    let result = apply_migrations_to_shadow_pg(&shadow_pool, migrations_dir).await;

    // Introspect the shadow database
    let schema = match result {
        Ok(()) => introspect::introspect_postgres(&shadow_pool, "public")
            .await
            .map_err(|e| ShadowError::Introspect(format!("Failed to introspect shadow: {e}"))),
        Err(e) => {
            // Clean up on failure
            shadow_pool.close().await;
            let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{shadow_db_name}""#))
                .execute(&server_pool)
                .await;
            return Err(e);
        }
    };

    // Clean up: disconnect from shadow and drop it
    shadow_pool.close().await;

    // Need a small delay for connection to fully close before dropping
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{shadow_db_name}""#))
        .execute(&server_pool)
        .await;

    server_pool.close().await;

    schema
}

#[cfg(feature = "postgres")]
async fn apply_migrations_to_shadow_pg(
    pool: &PgPool,
    migrations_dir: &Path,
) -> Result<(), ShadowError> {
    if !migrations_dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(migrations_dir)
        .map_err(|e| ShadowError::Io(e.to_string()))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .filter(|e| e.path().join("migration.sql").exists())
        .collect();

    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let sql_path = entry.path().join("migration.sql");
        let sql = std::fs::read_to_string(&sql_path)
            .map_err(|e| ShadowError::Io(format!("Failed to read {}: {e}", sql_path.display())))?;

        let migration_name = entry.file_name().to_string_lossy().to_string();

        sqlx::query(&sql).execute(pool).await.map_err(|e| {
            ShadowError::Migration(format!("Migration '{migration_name}' failed: {e}"))
        })?;
    }

    Ok(())
}

// ─── SQLite shadow database ──────────────────────────────────────

/// Run the shadow database diffing process for `SQLite`.
///
/// For `SQLite`, the shadow database is simply a temporary file.
/// No server connection is needed -- we create a temp file, replay migrations,
/// introspect the result, then delete the temp file.
///
/// # Errors
///
/// Returns a [`ShadowError`] if any step of the shadow database process fails.
#[cfg(feature = "sqlite")]
pub async fn introspect_via_shadow_sqlite(
    migrations_dir: &Path,
) -> Result<ferriorm_core::schema::Schema, ShadowError> {
    use sqlx::SqlitePool;

    // Create a temporary file for the shadow database
    let shadow_path = std::env::temp_dir().join(format!(
        "_ferriorm_shadow_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let shadow_url = format!("sqlite://{}?mode=rwc", shadow_path.display());

    let shadow_pool = SqlitePool::connect(&shadow_url)
        .await
        .map_err(|e| ShadowError::Connection(format!("Failed to create SQLite shadow DB: {e}")))?;

    // Enable foreign keys
    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&shadow_pool)
        .await
        .map_err(|e| ShadowError::Migration(format!("Failed to enable foreign keys: {e}")))?;

    // Apply all existing migrations in order
    let result = apply_migrations_to_shadow_sqlite(&shadow_pool, migrations_dir).await;

    // Introspect the shadow database
    let schema = match result {
        Ok(()) => introspect::introspect_sqlite(&shadow_pool)
            .await
            .map_err(|e| ShadowError::Introspect(format!("Failed to introspect shadow: {e}"))),
        Err(e) => {
            shadow_pool.close().await;
            let _ = std::fs::remove_file(&shadow_path);
            return Err(e);
        }
    };

    // Clean up
    shadow_pool.close().await;
    let _ = std::fs::remove_file(&shadow_path);

    schema
}

#[cfg(feature = "sqlite")]
async fn apply_migrations_to_shadow_sqlite(
    pool: &sqlx::SqlitePool,
    migrations_dir: &Path,
) -> Result<(), ShadowError> {
    if !migrations_dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(migrations_dir)
        .map_err(|e| ShadowError::Io(e.to_string()))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .filter(|e| e.path().join("migration.sql").exists())
        .collect();

    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let sql_path = entry.path().join("migration.sql");
        let sql = std::fs::read_to_string(&sql_path)
            .map_err(|e| ShadowError::Io(format!("Failed to read {}: {e}", sql_path.display())))?;

        let migration_name = entry.file_name().to_string_lossy().to_string();

        // SQLite requires executing statements one at a time in some cases,
        // but sqlx::query with multiple statements should work for most DDL.
        sqlx::query(&sql).execute(pool).await.map_err(|e| {
            ShadowError::Migration(format!("Migration '{migration_name}' failed: {e}"))
        })?;
    }

    Ok(())
}

// ─── URL helpers (PostgreSQL) ────────────────────────────────────

/// Strip the database name from a `PostgreSQL` URL, connecting to the default 'postgres' database.
#[cfg(feature = "postgres")]
fn strip_database_from_url(url: &str) -> String {
    // postgres://user:pass@host:port/dbname -> postgres://user:pass@host:port/postgres
    if let Some(pos) = url.rfind('/') {
        let base = &url[..pos];
        // Check if there are query params after the db name
        let db_and_params = &url[pos + 1..];
        if let Some(q_pos) = db_and_params.find('?') {
            format!("{}/postgres?{}", base, &db_and_params[q_pos + 1..])
        } else {
            format!("{base}/postgres")
        }
    } else {
        url.to_string()
    }
}

/// Replace the database name in a `PostgreSQL` URL.
#[cfg(feature = "postgres")]
fn replace_database_in_url(url: &str, new_db: &str) -> String {
    if let Some(pos) = url.rfind('/') {
        let base = &url[..pos];
        let db_and_params = &url[pos + 1..];
        if let Some(q_pos) = db_and_params.find('?') {
            format!("{}/{new_db}?{}", base, &db_and_params[q_pos + 1..])
        } else {
            format!("{base}/{new_db}")
        }
    } else {
        url.to_string()
    }
}

#[derive(Debug)]
pub enum ShadowError {
    Connection(String),
    Create(String),
    Migration(String),
    Introspect(String),
    Io(String),
}

impl std::fmt::Display for ShadowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection(msg) => write!(f, "Shadow DB connection error: {msg}"),
            Self::Create(msg) => write!(f, "Shadow DB creation error: {msg}"),
            Self::Migration(msg) => write!(f, "Shadow DB migration error: {msg}"),
            Self::Introspect(msg) => write!(f, "Shadow DB introspection error: {msg}"),
            Self::Io(msg) => write!(f, "Shadow DB IO error: {msg}"),
        }
    }
}

impl std::error::Error for ShadowError {}

#[cfg(test)]
mod tests {
    #[cfg(feature = "postgres")]
    use super::*;

    #[test]
    #[cfg(feature = "postgres")]
    fn test_strip_database_from_url() {
        assert_eq!(
            strip_database_from_url("postgres://user:pass@localhost:5432/mydb"),
            "postgres://user:pass@localhost:5432/postgres"
        );
        assert_eq!(
            strip_database_from_url("postgres://localhost/mydb?sslmode=disable"),
            "postgres://localhost/postgres?sslmode=disable"
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_replace_database_in_url() {
        assert_eq!(
            replace_database_in_url("postgres://user:pass@localhost:5432/mydb", "shadow_123"),
            "postgres://user:pass@localhost:5432/shadow_123"
        );
    }
}
