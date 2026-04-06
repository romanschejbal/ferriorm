//! End-to-end migration tests using an in-memory SQLite database.
//!
//! These tests exercise the full migration lifecycle:
//! 1. Parse a schema
//! 2. Create a migration (generates SQL + snapshot)
//! 3. Connect to SQLite in-memory
//! 4. Apply the migration
//! 5. Verify the database state
//! 6. Modify the schema, create another migration, apply, verify

use ormx_migrate::{MigrationRunner, MigrationStrategy};
use sqlx::SqlitePool;

const SCHEMA_V1: &str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

enum Status {
  Active
  Inactive
}

model User {
  id        String   @id @default(uuid())
  email     String   @unique
  name      String?
  status    Status   @default(Active)
  age       Int      @default(0)
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  @@map("users")
}
"#;

const SCHEMA_V2: &str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

enum Status {
  Active
  Inactive
  Suspended
}

model User {
  id        String   @id @default(uuid())
  email     String   @unique
  name      String?
  status    Status   @default(Active)
  age       Int      @default(0)
  bio       String?
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  @@map("users")
}

model Post {
  id        String   @id @default(uuid())
  title     String
  content   String?
  authorId  String
  author    User     @relation(fields: [authorId], references: [id])
  createdAt DateTime @default(now())

  @@map("posts")
}
"#;

/// Helper: connect to an in-memory SQLite database.
async fn sqlite_memory_pool() -> SqlitePool {
    SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite")
}

/// Helper: query sqlite_master for table names.
async fn get_table_names(pool: &SqlitePool) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .expect("query sqlite_master");
    rows.into_iter().map(|r| r.0).collect()
}

/// Helper: query column names for a given table.
async fn get_column_names(pool: &SqlitePool, table: &str) -> Vec<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await
            .expect("query pragma_table_info");
    rows.into_iter().map(|r| r.0).collect()
}

// ─── Snapshot strategy tests ────────────────────────────────────────

#[tokio::test]
async fn snapshot_create_initial_migration() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    // Create the first migration
    let result = runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration should succeed");

    let migration_dir = result.expect("should produce a migration (not None)");

    // Verify the migration directory was created
    assert!(
        migration_dir.exists(),
        "Migration directory should exist at {}",
        migration_dir.display()
    );

    // Verify migration.sql exists and contains CREATE TABLE
    let sql_path = migration_dir.join("migration.sql");
    assert!(sql_path.exists(), "migration.sql should exist");

    let sql_content = std::fs::read_to_string(&sql_path).expect("read migration.sql");
    assert!(
        sql_content.contains("CREATE TABLE \"users\""),
        "SQL should contain CREATE TABLE for users. Got:\n{sql_content}"
    );
    assert!(
        sql_content.contains("\"id\" TEXT NOT NULL"),
        "SQL should contain id column definition. Got:\n{sql_content}"
    );
    assert!(
        sql_content.contains("\"email\" TEXT NOT NULL"),
        "SQL should contain email column. Got:\n{sql_content}"
    );
    assert!(
        sql_content.contains("UNIQUE"),
        "SQL should contain UNIQUE for email. Got:\n{sql_content}"
    );
    assert!(
        sql_content.contains("PRIMARY KEY"),
        "SQL should contain PRIMARY KEY. Got:\n{sql_content}"
    );

    // Verify snapshot file exists
    let snapshot_path = migration_dir.join("_schema_snapshot.json");
    assert!(snapshot_path.exists(), "_schema_snapshot.json should exist");

    let snapshot_json = std::fs::read_to_string(&snapshot_path).expect("read snapshot");
    let snapshot_value: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot should be valid JSON");
    assert!(
        snapshot_value.get("models").is_some(),
        "Snapshot should contain models key"
    );
    assert!(
        snapshot_value.get("enums").is_some(),
        "Snapshot should contain enums key"
    );
}

#[tokio::test]
async fn snapshot_apply_migration_creates_table() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    // Create migration
    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration")
        .expect("migration should not be None");

    // Connect to SQLite and apply
    let pool = sqlite_memory_pool().await;
    let applied = runner
        .apply_pending_sqlite(&pool)
        .await
        .expect("apply_pending_sqlite should succeed");

    assert_eq!(applied.len(), 1, "One migration should have been applied");
    assert!(
        applied[0].contains("init"),
        "Applied migration name should contain 'init', got: {}",
        applied[0]
    );

    // Verify the table was created
    let tables = get_table_names(&pool).await;
    assert!(
        tables.contains(&"users".to_string()),
        "users table should exist. Tables found: {tables:?}"
    );

    // Verify columns
    let columns = get_column_names(&pool, "users").await;
    assert!(columns.contains(&"id".to_string()), "should have id column");
    assert!(
        columns.contains(&"email".to_string()),
        "should have email column"
    );
    assert!(
        columns.contains(&"name".to_string()),
        "should have name column"
    );
    assert!(
        columns.contains(&"status".to_string()),
        "should have status column"
    );
    assert!(
        columns.contains(&"age".to_string()),
        "should have age column"
    );
    assert!(
        columns.contains(&"created_at".to_string()),
        "should have created_at column"
    );
    assert!(
        columns.contains(&"updated_at".to_string()),
        "should have updated_at column"
    );
}

#[tokio::test]
async fn snapshot_migration_status_after_apply() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration")
        .expect("should produce migration");

    let pool = sqlite_memory_pool().await;
    runner.apply_pending_sqlite(&pool).await.expect("apply");

    // Check status
    let statuses = runner
        .status_sqlite(&pool)
        .await
        .expect("status_sqlite should succeed");

    assert_eq!(statuses.len(), 1, "Should have one migration in status");
    assert!(statuses[0].applied, "Migration should be marked as applied");
    assert!(
        statuses[0].applied_at.is_some(),
        "Applied migration should have a timestamp"
    );
    assert!(
        statuses[0].name.contains("init"),
        "Migration name should contain 'init'"
    );
}

#[tokio::test]
async fn snapshot_second_migration_adds_column_and_table() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");
    let schema_v2 = ormx_parser::parse_and_validate(SCHEMA_V2).expect("parse v2");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    // Create and apply first migration
    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration v1")
        .expect("v1 migration");

    let pool = sqlite_memory_pool().await;
    runner.apply_pending_sqlite(&pool).await.expect("apply v1");

    // Create second migration (schema diff)
    let v2_migration = runner
        .create_migration(&schema_v2, "add_posts_and_bio", None)
        .await
        .expect("create_migration v2");

    let v2_dir = v2_migration.expect("v2 should have changes");

    // Verify the second migration SQL contains the expected changes
    let v2_sql = std::fs::read_to_string(v2_dir.join("migration.sql")).expect("read v2 sql");

    // Should add bio column to users
    assert!(
        v2_sql.contains("ALTER TABLE \"users\" ADD COLUMN"),
        "v2 SQL should contain ALTER TABLE ADD COLUMN. Got:\n{v2_sql}"
    );
    assert!(
        v2_sql.contains("\"bio\""),
        "v2 SQL should add bio column. Got:\n{v2_sql}"
    );

    // Should create posts table
    assert!(
        v2_sql.contains("CREATE TABLE \"posts\""),
        "v2 SQL should create posts table. Got:\n{v2_sql}"
    );

    // Should mention new enum variant (as a comment for SQLite)
    assert!(
        v2_sql.contains("Suspended") || v2_sql.contains("suspended"),
        "v2 SQL should reference new Suspended enum variant. Got:\n{v2_sql}"
    );

    // Apply second migration
    let applied = runner.apply_pending_sqlite(&pool).await.expect("apply v2");

    assert_eq!(applied.len(), 1, "Only one new migration should be applied");

    // Verify database state
    let tables = get_table_names(&pool).await;
    assert!(
        tables.contains(&"users".to_string()),
        "users table should still exist"
    );
    assert!(
        tables.contains(&"posts".to_string()),
        "posts table should now exist. Tables: {tables:?}"
    );

    // Verify bio column was added to users
    let user_columns = get_column_names(&pool, "users").await;
    assert!(
        user_columns.contains(&"bio".to_string()),
        "users should now have bio column. Columns: {user_columns:?}"
    );

    // Verify posts columns
    let post_columns = get_column_names(&pool, "posts").await;
    assert!(
        post_columns.contains(&"id".to_string()),
        "posts should have id"
    );
    assert!(
        post_columns.contains(&"title".to_string()),
        "posts should have title"
    );
    assert!(
        post_columns.contains(&"content".to_string()),
        "posts should have content"
    );
    assert!(
        post_columns.contains(&"author_id".to_string()),
        "posts should have author_id"
    );

    // Verify migration status shows both applied
    let statuses = runner.status_sqlite(&pool).await.expect("status_sqlite");
    assert_eq!(statuses.len(), 2, "Should have two migrations");
    assert!(statuses[0].applied, "First migration should be applied");
    assert!(statuses[1].applied, "Second migration should be applied");
}

#[tokio::test]
async fn snapshot_no_changes_returns_none() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    // Create first migration
    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration")
        .expect("should create initial migration");

    // Try to create another migration with the same schema -- should return None
    let result = runner
        .create_migration(&schema_v1, "no_changes", None)
        .await
        .expect("create_migration should succeed");

    assert!(
        result.is_none(),
        "Creating migration with identical schema should return None (no changes)"
    );
}

#[tokio::test]
async fn snapshot_apply_is_idempotent() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration")
        .expect("migration");

    let pool = sqlite_memory_pool().await;

    // Apply once
    let first = runner
        .apply_pending_sqlite(&pool)
        .await
        .expect("first apply");
    assert_eq!(first.len(), 1);

    // Apply again -- should be a no-op
    let second = runner
        .apply_pending_sqlite(&pool)
        .await
        .expect("second apply");
    assert_eq!(
        second.len(),
        0,
        "Second apply should find no pending migrations"
    );
}

#[tokio::test]
async fn snapshot_can_insert_and_query_data_after_migration() {
    let schema_v1 = ormx_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ormx_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::Snapshot,
    );

    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration")
        .expect("migration");

    let pool = sqlite_memory_pool().await;
    runner.apply_pending_sqlite(&pool).await.expect("apply");

    // Insert a row using raw SQL
    sqlx::query(
        "INSERT INTO \"users\" (\"id\", \"email\", \"name\", \"status\", \"age\", \"created_at\", \"updated_at\") VALUES ('user-1', 'alice@example.com', 'Alice', 'active', 30, datetime('now'), datetime('now'))"
    )
    .execute(&pool)
    .await
    .expect("insert user");

    // Query it back
    let row: (String, String, Option<String>, String, i32) = sqlx::query_as(
        "SELECT \"id\", \"email\", \"name\", \"status\", \"age\" FROM \"users\" WHERE \"id\" = 'user-1'"
    )
    .fetch_one(&pool)
    .await
    .expect("query user");

    assert_eq!(row.0, "user-1");
    assert_eq!(row.1, "alice@example.com");
    assert_eq!(row.2, Some("Alice".to_string()));
    assert_eq!(row.3, "active");
    assert_eq!(row.4, 30);

    // Verify UNIQUE constraint works
    let duplicate_result = sqlx::query(
        "INSERT INTO \"users\" (\"id\", \"email\", \"status\", \"age\", \"created_at\", \"updated_at\") VALUES ('user-2', 'alice@example.com', 'active', 25, datetime('now'), datetime('now'))"
    )
    .execute(&pool)
    .await;

    assert!(
        duplicate_result.is_err(),
        "Inserting duplicate email should fail due to UNIQUE constraint"
    );
}
