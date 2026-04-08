//! Schema snapshot serialization for migration diffing.
//!
//! Each migration directory can contain a `_schema_snapshot.json` file that
//! captures the full [`Schema`] at that point in time. This module provides
//! [`serialize`] and [`deserialize`] for JSON round-tripping, plus
//! [`load_latest_snapshot`] to find the most recent snapshot in the migrations
//! directory. Used by the `Snapshot` migration strategy as an alternative to
//! the shadow database approach.

use ferriorm_core::schema::Schema;
use std::path::Path;

/// Serialize a schema to JSON for storage alongside migrations.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] if serialization fails.
pub fn serialize(schema: &Schema) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(schema)
}

/// Deserialize a schema from a JSON snapshot.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] if deserialization fails.
pub fn deserialize(json: &str) -> Result<Schema, serde_json::Error> {
    serde_json::from_str(json)
}

/// Load the most recent schema snapshot from the migrations directory.
#[must_use]
pub fn load_latest_snapshot(migrations_dir: &Path) -> Option<Schema> {
    let mut entries: Vec<_> = std::fs::read_dir(migrations_dir)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();

    entries.sort_by_key(std::fs::DirEntry::file_name);

    // Find the latest migration directory with a snapshot
    for entry in entries.iter().rev() {
        let snapshot_path = entry.path().join("_schema_snapshot.json");
        if snapshot_path.exists() {
            let json = std::fs::read_to_string(&snapshot_path).ok()?;
            return deserialize(&json).ok();
        }
    }

    None
}

/// Create an empty schema (for the first migration).
#[must_use]
pub fn empty_schema(provider: ferriorm_core::types::DatabaseProvider) -> Schema {
    Schema {
        datasource: ferriorm_core::schema::DatasourceConfig {
            name: "db".into(),
            provider,
            url: String::new(),
        },
        generators: vec![],
        enums: vec![],
        models: vec![],
    }
}
