//! Migration runner: orchestrates creating, applying, and managing migrations.

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};

use crate::{diff, shadow, snapshot, sql, state};

/// How to determine the "current applied state" for diffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStrategy {
    /// Create a temporary database, replay all migrations, introspect.
    /// Accurate even when migration.sql files are manually edited.
    /// Requires a database connection.
    ShadowDatabase,

    /// Use the JSON schema snapshot stored alongside each migration.
    /// Fast and offline, but drifts if migration.sql files are edited.
    Snapshot,
}

impl Default for MigrationStrategy {
    fn default() -> Self {
        Self::ShadowDatabase
    }
}

pub struct MigrationRunner {
    migrations_dir: PathBuf,
    provider: ormx_core::types::DatabaseProvider,
    strategy: MigrationStrategy,
}

impl MigrationRunner {
    pub fn new(
        migrations_dir: PathBuf,
        provider: ormx_core::types::DatabaseProvider,
        strategy: MigrationStrategy,
    ) -> Self {
        Self {
            migrations_dir,
            provider,
            strategy,
        }
    }

    /// Create a new migration by diffing the current schema against the applied state.
    ///
    /// For `ShadowDatabase` strategy: needs `database_url` to create a temp DB.
    /// For `Snapshot` strategy: `database_url` is not used.
    pub async fn create_migration(
        &self,
        current_schema: &ormx_core::schema::Schema,
        name: &str,
        database_url: Option<&str>,
    ) -> Result<Option<PathBuf>, MigrateError> {
        std::fs::create_dir_all(&self.migrations_dir)
            .map_err(|e| MigrateError::Io(format!("Failed to create migrations dir: {e}")))?;

        // Determine the "applied state" based on strategy
        let previous = match self.strategy {
            MigrationStrategy::ShadowDatabase => {
                let url = database_url.ok_or_else(|| {
                    MigrateError::Io("Shadow database strategy requires a database URL".to_string())
                })?;

                let has_migrations = self
                    .list_migrations()?
                    .iter()
                    .any(|m| m.join("migration.sql").exists());

                if has_migrations {
                    shadow::introspect_via_shadow(url, &self.migrations_dir)
                        .await
                        .map_err(|e| MigrateError::Database(e.to_string()))?
                } else {
                    snapshot::empty_schema(self.provider)
                }
            }
            MigrationStrategy::Snapshot => snapshot::load_latest_snapshot(&self.migrations_dir)
                .unwrap_or_else(|| snapshot::empty_schema(self.provider)),
        };

        let steps = diff::diff_schemas(&previous, current_schema, self.provider);

        if steps.is_empty() {
            return Ok(None);
        }

        // Generate SQL
        let renderer = sql::renderer_for(self.provider);
        let sql_content = renderer.render(&steps);

        // Create migration directory
        let seq = self.next_sequence_number();
        let dir_name = format!("{:04}_{}", seq, sanitize_name(name));
        let migration_dir = self.migrations_dir.join(&dir_name);
        std::fs::create_dir_all(&migration_dir)
            .map_err(|e| MigrateError::Io(format!("Failed to create migration dir: {e}")))?;

        // Write migration.sql
        let sql_path = migration_dir.join("migration.sql");
        std::fs::write(&sql_path, &sql_content)
            .map_err(|e| MigrateError::Io(format!("Failed to write migration.sql: {e}")))?;

        // Always write a schema snapshot (useful for snapshot mode and as documentation)
        let snapshot_json = snapshot::serialize(current_schema)
            .map_err(|e| MigrateError::Io(format!("Failed to serialize schema: {e}")))?;
        let snapshot_path = migration_dir.join("_schema_snapshot.json");
        std::fs::write(&snapshot_path, &snapshot_json)
            .map_err(|e| MigrateError::Io(format!("Failed to write snapshot: {e}")))?;

        Ok(Some(migration_dir))
    }

    /// Apply all pending migrations to the database.
    pub async fn apply_pending(&self, pool: &PgPool) -> Result<Vec<String>, MigrateError> {
        state::ensure_table(pool)
            .await
            .map_err(|e| MigrateError::Database(e.to_string()))?;

        let applied = state::get_applied(pool)
            .await
            .map_err(|e| MigrateError::Database(e.to_string()))?;

        let applied_names: std::collections::HashSet<String> =
            applied.iter().map(|m| m.name.clone()).collect();

        let pending = self.list_migrations()?;
        let mut applied_new = Vec::new();

        for migration in pending {
            let name = migration.file_name().unwrap().to_string_lossy().to_string();

            if applied_names.contains(&name) {
                // Verify checksum
                let sql_path = migration.join("migration.sql");
                let sql = std::fs::read_to_string(&sql_path).map_err(|e| {
                    MigrateError::Io(format!("Failed to read {}: {e}", sql_path.display()))
                })?;
                let checksum = compute_checksum(&sql);

                if let Some(existing) = applied.iter().find(|m| m.name == name) {
                    if existing.checksum != checksum {
                        return Err(MigrateError::ChecksumMismatch {
                            migration: name,
                            expected: existing.checksum.clone(),
                            actual: checksum,
                        });
                    }
                }
                continue;
            }

            // Apply this migration
            let sql_path = migration.join("migration.sql");
            let sql = std::fs::read_to_string(&sql_path).map_err(|e| {
                MigrateError::Io(format!("Failed to read {}: {e}", sql_path.display()))
            })?;
            let checksum = compute_checksum(&sql);

            sqlx::query(&sql)
                .execute(pool)
                .await
                .map_err(|e| MigrateError::Database(format!("Migration '{name}' failed: {e}")))?;

            state::mark_applied(pool, &name, &checksum)
                .await
                .map_err(|e| MigrateError::Database(e.to_string()))?;

            applied_new.push(name);
        }

        Ok(applied_new)
    }

    /// Get the status of all migrations.
    pub async fn status(&self, pool: &PgPool) -> Result<Vec<MigrationStatus>, MigrateError> {
        state::ensure_table(pool)
            .await
            .map_err(|e| MigrateError::Database(e.to_string()))?;

        let applied = state::get_applied(pool)
            .await
            .map_err(|e| MigrateError::Database(e.to_string()))?;

        let applied_map: std::collections::HashMap<String, &state::AppliedMigration> =
            applied.iter().map(|m| (m.name.clone(), m)).collect();

        let all = self.list_migrations()?;
        let mut statuses = Vec::new();

        for migration in all {
            let name = migration.file_name().unwrap().to_string_lossy().to_string();

            let status = if let Some(m) = applied_map.get(&name) {
                MigrationStatus {
                    name: name.clone(),
                    applied: true,
                    applied_at: Some(m.applied_at),
                }
            } else {
                MigrationStatus {
                    name: name.clone(),
                    applied: false,
                    applied_at: None,
                }
            };
            statuses.push(status);
        }

        Ok(statuses)
    }

    /// List all migration directories in order.
    fn list_migrations(&self) -> Result<Vec<PathBuf>, MigrateError> {
        if !self.migrations_dir.exists() {
            return Ok(vec![]);
        }

        let mut entries: Vec<PathBuf> = std::fs::read_dir(&self.migrations_dir)
            .map_err(|e| MigrateError::Io(e.to_string()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| e.path().join("migration.sql").exists())
            .map(|e| e.path())
            .collect();

        entries.sort();
        Ok(entries)
    }

    fn next_sequence_number(&self) -> u32 {
        self.list_migrations()
            .ok()
            .map(|m| m.len() as u32 + 1)
            .unwrap_or(1)
    }
}

#[derive(Debug)]
pub struct MigrationStatus {
    pub name: String,
    pub applied: bool,
    pub applied_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_lowercase()
}

fn compute_checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug)]
pub enum MigrateError {
    Io(String),
    Database(String),
    ChecksumMismatch {
        migration: String,
        expected: String,
        actual: String,
    },
    NoChanges,
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "IO error: {msg}"),
            Self::Database(msg) => write!(f, "Database error: {msg}"),
            Self::ChecksumMismatch {
                migration,
                expected,
                actual,
            } => write!(
                f,
                "Checksum mismatch for migration '{migration}': expected {expected}, got {actual}. The migration file was modified after being applied."
            ),
            Self::NoChanges => write!(f, "No schema changes detected"),
        }
    }
}

impl std::error::Error for MigrateError {}
