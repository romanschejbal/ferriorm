#![allow(clippy::pedantic)]

//! End-to-end migration tests using the `ShadowDatabase` strategy with SQLite.
//!
//! The shadow database strategy creates a temporary SQLite file, replays all
//! existing migrations into it, introspects the result, and then diffs against
//! the current schema. This tests the full shadow DB flow.

use ferriorm_migrate::{MigrationRunner, MigrationStrategy};
use sqlx::SqlitePool;

const SCHEMA_V1: &str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

model Category {
  id   String @id @default(uuid())
  name String @unique

  @@map("categories")
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

model Category {
  id          String  @id @default(uuid())
  name        String  @unique
  description String?

  @@map("categories")
}

model Product {
  id         String   @id @default(uuid())
  name       String
  price      Int      @default(0)
  categoryId String
  category   Category @relation(fields: [categoryId], references: [id])
  createdAt  DateTime @default(now())

  @@map("products")
  @@index([categoryId])
}
"#;

async fn sqlite_memory_pool() -> SqlitePool {
    SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite")
}

async fn get_table_names(pool: &SqlitePool) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .expect("query sqlite_master");
    rows.into_iter().map(|r| r.0).collect()
}

async fn get_column_names(pool: &SqlitePool, table: &str) -> Vec<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await
            .expect("query pragma_table_info");
    rows.into_iter().map(|r| r.0).collect()
}

#[tokio::test]
async fn shadow_create_initial_migration() {
    let schema_v1 = ferriorm_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ferriorm_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::ShadowDatabase,
    );

    // First migration -- no existing migrations, so shadow sees empty schema
    let result = runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create_migration should succeed");

    let migration_dir = result.expect("should produce a migration");

    // Verify migration files
    let sql_path = migration_dir.join("migration.sql");
    assert!(sql_path.exists(), "migration.sql should exist");

    let sql = std::fs::read_to_string(&sql_path).expect("read sql");
    assert!(
        sql.contains("CREATE TABLE \"categories\""),
        "SQL should create categories table. Got:\n{sql}"
    );
    assert!(
        sql.contains("\"id\" TEXT NOT NULL"),
        "SQL should define id column. Got:\n{sql}"
    );
    assert!(
        sql.contains("\"name\" TEXT NOT NULL"),
        "SQL should define name column. Got:\n{sql}"
    );
    assert!(
        sql.contains("UNIQUE"),
        "name column should be UNIQUE. Got:\n{sql}"
    );

    let snapshot_path = migration_dir.join("_schema_snapshot.json");
    assert!(snapshot_path.exists(), "_schema_snapshot.json should exist");
}

#[tokio::test]
async fn shadow_apply_and_verify() {
    let schema_v1 = ferriorm_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ferriorm_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::ShadowDatabase,
    );

    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create")
        .expect("migration");

    let pool = sqlite_memory_pool().await;
    let applied = runner.apply_pending_sqlite(&pool).await.expect("apply");

    assert_eq!(applied.len(), 1);

    let tables = get_table_names(&pool).await;
    assert!(
        tables.contains(&"categories".to_string()),
        "categories table should exist"
    );
}

#[tokio::test]
async fn shadow_second_migration_via_introspection() {
    let schema_v1 = ferriorm_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");
    let schema_v2 = ferriorm_parser::parse_and_validate(SCHEMA_V2).expect("parse v2");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ferriorm_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::ShadowDatabase,
    );

    // Create and apply first migration
    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create v1")
        .expect("v1 migration");

    // For the second migration with shadow strategy, the runner:
    // 1. Creates a temp SQLite file
    // 2. Replays the init migration
    // 3. Introspects the result (gets v1 schema)
    // 4. Diffs v1 introspected vs v2 current
    let v2_result = runner
        .create_migration(&schema_v2, "add_products", None)
        .await
        .expect("create v2");

    let v2_dir = v2_result.expect("v2 should have changes");

    let v2_sql = std::fs::read_to_string(v2_dir.join("migration.sql")).expect("read v2 sql");

    // Should create products table
    assert!(
        v2_sql.contains("CREATE TABLE \"products\""),
        "v2 should create products table. Got:\n{v2_sql}"
    );

    // Should add description column to categories
    // (Note: shadow introspection may produce the diff slightly differently
    //  than snapshot, but the intent is the same)
    assert!(
        v2_sql.contains("\"description\""),
        "v2 should reference description column. Got:\n{v2_sql}"
    );

    // Apply both migrations to a fresh database
    let pool = sqlite_memory_pool().await;
    let applied = runner.apply_pending_sqlite(&pool).await.expect("apply all");
    assert_eq!(
        applied.len(),
        2,
        "Both migrations should be applied. Applied: {applied:?}"
    );

    // Verify final state
    let tables = get_table_names(&pool).await;
    assert!(tables.contains(&"categories".to_string()));
    assert!(tables.contains(&"products".to_string()));

    let cat_cols = get_column_names(&pool, "categories").await;
    assert!(cat_cols.contains(&"description".to_string()));

    let prod_cols = get_column_names(&pool, "products").await;
    assert!(prod_cols.contains(&"id".to_string()));
    assert!(prod_cols.contains(&"name".to_string()));
    assert!(prod_cols.contains(&"price".to_string()));
    assert!(prod_cols.contains(&"category_id".to_string()));

    // Verify status
    let statuses = runner.status_sqlite(&pool).await.expect("status");
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|s| s.applied));
}

#[tokio::test]
async fn shadow_detects_no_changes() {
    let schema_v1 = ferriorm_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ferriorm_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::ShadowDatabase,
    );

    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create")
        .expect("migration");

    // Shadow replays init migration, introspects, diffs against v1 -- should find no changes
    let result = runner
        .create_migration(&schema_v1, "nothing", None)
        .await
        .expect("create_migration should succeed");

    assert!(
        result.is_none(),
        "Shadow strategy should detect no changes when schema hasn't changed"
    );
}

#[tokio::test]
async fn shadow_can_insert_data_after_multi_migration() {
    let schema_v1 = ferriorm_parser::parse_and_validate(SCHEMA_V1).expect("parse v1");
    let schema_v2 = ferriorm_parser::parse_and_validate(SCHEMA_V2).expect("parse v2");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let migrations_dir = tmp_dir.path().join("migrations");

    let runner = MigrationRunner::new(
        migrations_dir.clone(),
        ferriorm_core::types::DatabaseProvider::SQLite,
        MigrationStrategy::ShadowDatabase,
    );

    runner
        .create_migration(&schema_v1, "init", None)
        .await
        .expect("create v1")
        .expect("v1");

    runner
        .create_migration(&schema_v2, "add_products", None)
        .await
        .expect("create v2")
        .expect("v2");

    let pool = sqlite_memory_pool().await;
    runner.apply_pending_sqlite(&pool).await.expect("apply all");

    // Insert a category
    sqlx::query(
        "INSERT INTO \"categories\" (\"id\", \"name\", \"description\") VALUES ('cat-1', 'Electronics', 'Gadgets and devices')",
    )
    .execute(&pool)
    .await
    .expect("insert category");

    // Insert a product referencing the category
    sqlx::query(
        "INSERT INTO \"products\" (\"id\", \"name\", \"price\", \"category_id\", \"created_at\") VALUES ('prod-1', 'Laptop', 999, 'cat-1', datetime('now'))",
    )
    .execute(&pool)
    .await
    .expect("insert product");

    // Query with JOIN
    let row: (String, String, i32, String) = sqlx::query_as(
        "SELECT p.\"id\", p.\"name\", p.\"price\", c.\"name\" FROM \"products\" p JOIN \"categories\" c ON p.\"category_id\" = c.\"id\" WHERE p.\"id\" = 'prod-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("query product with category");

    assert_eq!(row.0, "prod-1");
    assert_eq!(row.1, "Laptop");
    assert_eq!(row.2, 999);
    assert_eq!(row.3, "Electronics");
}
