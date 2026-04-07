//! End-to-end tests for Tier 1 features:
//!   - Pool configuration (PoolConfig, connect_with_config)
//!   - Raw SQL helpers (raw_execute_sqlite, raw_fetch_all/one/optional_sqlite, sqlite_pool)
//!   - Aggregates (AVG, SUM, MIN, MAX via the same SQL patterns as generated code)
//!   - Select / partial loading (column subsets via the same SQL patterns as generated code)

use ferriorm_runtime::client::{DatabaseClient, PoolConfig};
// ─── Test structs ────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct TestUser {
    id: String,
    email: String,
    name: Option<String>,
    age: i64,
    active: i64, // SQLite boolean
}

#[derive(Debug, sqlx::FromRow)]
struct AggResult {
    #[sqlx(default)]
    avg_age: Option<f64>,
    #[sqlx(default)]
    sum_age: Option<i64>,
    #[sqlx(default)]
    min_age: Option<i64>,
    #[sqlx(default)]
    max_age: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct PartialUser {
    #[sqlx(default)]
    id: Option<String>,
    #[sqlx(default)]
    email: Option<String>,
    #[sqlx(default)]
    name: Option<String>,
    #[sqlx(default)]
    age: Option<i64>,
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Create an in-memory SQLite database via DatabaseClient and set up a users table.
async fn setup_client() -> DatabaseClient {
    let client = DatabaseClient::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");

    client
        .raw_execute_sqlite(
            r#"CREATE TABLE "users" (
                "id" TEXT NOT NULL PRIMARY KEY,
                "email" TEXT NOT NULL UNIQUE,
                "name" TEXT,
                "age" INTEGER NOT NULL DEFAULT 0,
                "active" INTEGER NOT NULL DEFAULT 1,
                "created_at" TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        )
        .await
        .expect("create users table");

    client
}

/// Insert a user row via raw SQL.
async fn insert_user(
    client: &DatabaseClient,
    id: &str,
    email: &str,
    name: Option<&str>,
    age: i64,
    active: bool,
) {
    let active_int: i64 = if active { 1 } else { 0 };
    let pool = client.sqlite_pool().expect("sqlite pool");
    sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))"#,
    )
    .bind(id)
    .bind(email)
    .bind(name)
    .bind(age)
    .bind(active_int)
    .execute(pool)
    .await
    .expect("insert user");
}

/// Insert a user row with a specific created_at timestamp.
async fn insert_user_with_timestamp(
    client: &DatabaseClient,
    id: &str,
    email: &str,
    name: Option<&str>,
    age: i64,
    created_at: &str,
) {
    let pool = client.sqlite_pool().expect("sqlite pool");
    sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, 1, ?)"#,
    )
    .bind(id)
    .bind(email)
    .bind(name)
    .bind(age)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("insert user with timestamp");
}

// ═══════════════════════════════════════════════════════════════════════
// Pool Configuration Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_pool_config_default() {
    let config = PoolConfig::default();
    assert!(config.max_connections.is_none());
    assert!(config.min_connections.is_none());
    assert!(config.idle_timeout.is_none());
    assert!(config.max_lifetime.is_none());
    assert!(config.acquire_timeout.is_none());
}

#[tokio::test]
async fn test_connect_with_pool_config() {
    let config = PoolConfig {
        max_connections: Some(5),
        ..Default::default()
    };
    let client = DatabaseClient::connect_with_config("sqlite::memory:", &config)
        .await
        .expect("connect with pool config");

    // Verify connection works by executing a simple query
    let affected = client
        .raw_execute_sqlite("CREATE TABLE test_table (id INTEGER PRIMARY KEY)")
        .await
        .expect("create table");
    assert_eq!(affected, 0); // DDL returns 0 affected rows

    client.disconnect().await;
}

#[tokio::test]
async fn test_connect_with_default_config() {
    let config = PoolConfig::default();
    let client = DatabaseClient::connect_with_config("sqlite::memory:", &config)
        .await
        .expect("connect with default config");

    // Should work identically to regular connect
    let affected = client
        .raw_execute_sqlite("CREATE TABLE test_table (id INTEGER PRIMARY KEY)")
        .await
        .expect("create table");
    assert_eq!(affected, 0);

    client.disconnect().await;
}

// ═══════════════════════════════════════════════════════════════════════
// Raw SQL Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_raw_execute_sqlite() {
    let client = DatabaseClient::connect("sqlite::memory:")
        .await
        .expect("connect");

    // Create table manually
    client
        .raw_execute_sqlite(
            r#"CREATE TABLE "items" ("id" INTEGER PRIMARY KEY, "name" TEXT NOT NULL)"#,
        )
        .await
        .expect("create table");

    // Use raw_execute_sqlite to INSERT
    let affected = client
        .raw_execute_sqlite(r#"INSERT INTO "items" ("id", "name") VALUES (1, 'Widget')"#)
        .await
        .expect("insert");
    assert_eq!(affected, 1);

    // Verify row exists
    let pool = client.sqlite_pool().expect("pool");
    let row: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "items""#)
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(row.0, 1);
}

#[derive(Debug, sqlx::FromRow)]
struct Item {
    id: i64,
    name: String,
}

#[tokio::test]
async fn test_raw_fetch_all_sqlite() {
    let client = DatabaseClient::connect("sqlite::memory:")
        .await
        .expect("connect");

    client
        .raw_execute_sqlite(
            r#"CREATE TABLE "items" ("id" INTEGER PRIMARY KEY, "name" TEXT NOT NULL)"#,
        )
        .await
        .expect("create table");
    client
        .raw_execute_sqlite(r#"INSERT INTO "items" ("id", "name") VALUES (1, 'Alpha')"#)
        .await
        .expect("insert 1");
    client
        .raw_execute_sqlite(r#"INSERT INTO "items" ("id", "name") VALUES (2, 'Beta')"#)
        .await
        .expect("insert 2");
    client
        .raw_execute_sqlite(r#"INSERT INTO "items" ("id", "name") VALUES (3, 'Gamma')"#)
        .await
        .expect("insert 3");

    let items: Vec<Item> = client
        .raw_fetch_all_sqlite(r#"SELECT "id", "name" FROM "items" ORDER BY "id""#)
        .await
        .expect("fetch all");

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].name, "Alpha");
    assert_eq!(items[1].name, "Beta");
    assert_eq!(items[2].name, "Gamma");
}

#[tokio::test]
async fn test_raw_fetch_one_sqlite() {
    let client = DatabaseClient::connect("sqlite::memory:")
        .await
        .expect("connect");

    client
        .raw_execute_sqlite(
            r#"CREATE TABLE "items" ("id" INTEGER PRIMARY KEY, "name" TEXT NOT NULL)"#,
        )
        .await
        .expect("create table");
    client
        .raw_execute_sqlite(r#"INSERT INTO "items" ("id", "name") VALUES (1, 'Only')"#)
        .await
        .expect("insert");

    let item: Item = client
        .raw_fetch_one_sqlite(r#"SELECT "id", "name" FROM "items" WHERE "id" = 1"#)
        .await
        .expect("fetch one");

    assert_eq!(item.id, 1);
    assert_eq!(item.name, "Only");
}

#[tokio::test]
async fn test_raw_fetch_optional_sqlite() {
    let client = DatabaseClient::connect("sqlite::memory:")
        .await
        .expect("connect");

    client
        .raw_execute_sqlite(
            r#"CREATE TABLE "items" ("id" INTEGER PRIMARY KEY, "name" TEXT NOT NULL)"#,
        )
        .await
        .expect("create table");
    client
        .raw_execute_sqlite(r#"INSERT INTO "items" ("id", "name") VALUES (1, 'Exists')"#)
        .await
        .expect("insert");

    // Existing row -> Some
    let found: Option<Item> = client
        .raw_fetch_optional_sqlite(r#"SELECT "id", "name" FROM "items" WHERE "id" = 1"#)
        .await
        .expect("fetch optional existing");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Exists");

    // Non-existing row -> None
    let missing: Option<Item> = client
        .raw_fetch_optional_sqlite(r#"SELECT "id", "name" FROM "items" WHERE "id" = 999"#)
        .await
        .expect("fetch optional missing");
    assert!(missing.is_none());
}

#[tokio::test]
async fn test_sqlite_pool_accessor() {
    let client = setup_client().await;

    // Get the pool directly
    let pool = client.sqlite_pool().expect("sqlite pool");

    // Use sqlx directly with .bind() for parameterized queries
    sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))"#,
    )
    .bind("u1")
    .bind("test@example.com")
    .bind("Test User")
    .bind(25i64)
    .bind(1i64)
    .execute(pool)
    .await
    .expect("insert via pool");

    let user: TestUser = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(pool)
    .await
    .expect("select via pool");

    assert_eq!(user.id, "u1");
    assert_eq!(user.email, "test@example.com");
    assert_eq!(user.name, Some("Test User".to_string()));
    assert_eq!(user.age, 25);
}

#[tokio::test]
async fn test_raw_execute_returns_affected_rows() {
    let client = setup_client().await;

    // INSERT multiple rows
    insert_user(&client, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&client, "u2", "b@test.com", Some("B"), 25, true).await;
    insert_user(&client, "u3", "c@test.com", Some("C"), 30, false).await;
    insert_user(&client, "u4", "d@test.com", Some("D"), 35, false).await;

    // DELETE inactive users, verify affected count
    let affected = client
        .raw_execute_sqlite(r#"DELETE FROM "users" WHERE "active" = 0"#)
        .await
        .expect("delete inactive");
    assert_eq!(affected, 2);

    // Verify remaining rows
    let remaining: Vec<TestUser> = client
        .raw_fetch_all_sqlite(
            r#"SELECT "id", "email", "name", "age", "active" FROM "users" ORDER BY "id""#,
        )
        .await
        .expect("fetch remaining");
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0].id, "u1");
    assert_eq!(remaining[1].id, "u2");
}

// ═══════════════════════════════════════════════════════════════════════
// Aggregate Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_aggregate_avg() {
    let client = setup_client().await;

    insert_user(&client, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&client, "u2", "b@test.com", Some("B"), 30, true).await;
    insert_user(&client, "u3", "c@test.com", Some("C"), 40, true).await;

    // Same SQL pattern as generated AggregateQuery::exec
    let result: AggResult = client
        .raw_fetch_one_sqlite(r#"SELECT AVG("age") as "avg_age" FROM "users" WHERE 1=1"#)
        .await
        .expect("aggregate avg");

    let avg = result.avg_age.expect("avg should not be NULL");
    assert!(
        (avg - 30.0).abs() < f64::EPSILON,
        "AVG should be 30.0, got {avg}"
    );
}

#[tokio::test]
async fn test_aggregate_sum() {
    let client = setup_client().await;

    insert_user(&client, "u1", "a@test.com", Some("A"), 10, true).await;
    insert_user(&client, "u2", "b@test.com", Some("B"), 20, true).await;
    insert_user(&client, "u3", "c@test.com", Some("C"), 30, true).await;

    let result: AggResult = client
        .raw_fetch_one_sqlite(r#"SELECT SUM("age") as "sum_age" FROM "users" WHERE 1=1"#)
        .await
        .expect("aggregate sum");

    let sum = result.sum_age.expect("sum should not be NULL");
    assert_eq!(sum, 60, "SUM should be 60, got {sum}");
}

#[tokio::test]
async fn test_aggregate_min_max() {
    let client = setup_client().await;

    insert_user(&client, "u1", "a@test.com", Some("A"), 15, true).await;
    insert_user(&client, "u2", "b@test.com", Some("B"), 25, true).await;
    insert_user(&client, "u3", "c@test.com", Some("C"), 50, true).await;

    let result: AggResult = client
        .raw_fetch_one_sqlite(
            r#"SELECT MIN("age") as "min_age", MAX("age") as "max_age" FROM "users" WHERE 1=1"#,
        )
        .await
        .expect("aggregate min/max");

    assert_eq!(result.min_age, Some(15));
    assert_eq!(result.max_age, Some(50));
}

#[tokio::test]
async fn test_aggregate_with_where_filter() {
    let client = setup_client().await;

    insert_user(&client, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&client, "u2", "b@test.com", Some("B"), 30, false).await;
    insert_user(&client, "u3", "c@test.com", Some("C"), 40, true).await;
    insert_user(&client, "u4", "d@test.com", Some("D"), 50, false).await;

    // Aggregate only active users (same WHERE pattern as generated code)
    let result: AggResult = client
        .raw_fetch_one_sqlite(
            r#"SELECT AVG("age") as "avg_age", SUM("age") as "sum_age" FROM "users" WHERE 1=1 AND "active" = 1"#,
        )
        .await
        .expect("aggregate with where");

    let avg = result.avg_age.expect("avg should not be NULL");
    assert!(
        (avg - 30.0).abs() < f64::EPSILON,
        "AVG of active users should be 30.0, got {avg}"
    );
    let sum = result.sum_age.expect("sum should not be NULL");
    assert_eq!(sum, 60, "SUM of active users should be 60, got {sum}");
}

#[tokio::test]
async fn test_aggregate_empty_table() {
    let client = setup_client().await;

    // No rows inserted -- aggregate on empty table returns NULL
    let result: AggResult = client
        .raw_fetch_one_sqlite(
            r#"SELECT AVG("age") as "avg_age", SUM("age") as "sum_age", MIN("age") as "min_age", MAX("age") as "max_age" FROM "users" WHERE 1=1"#,
        )
        .await
        .expect("aggregate empty table");

    assert!(
        result.avg_age.is_none(),
        "AVG of empty table should be None"
    );
    assert!(
        result.sum_age.is_none(),
        "SUM of empty table should be None"
    );
    assert!(
        result.min_age.is_none(),
        "MIN of empty table should be None"
    );
    assert!(
        result.max_age.is_none(),
        "MAX of empty table should be None"
    );
}

#[derive(Debug, sqlx::FromRow)]
struct DateTimeAggResult {
    #[sqlx(default)]
    min_created_at: Option<String>,
    #[sqlx(default)]
    max_created_at: Option<String>,
}

#[tokio::test]
async fn test_aggregate_min_max_datetime() {
    let client = setup_client().await;

    insert_user_with_timestamp(
        &client,
        "u1",
        "a@test.com",
        Some("A"),
        20,
        "2024-01-01 10:00:00",
    )
    .await;
    insert_user_with_timestamp(
        &client,
        "u2",
        "b@test.com",
        Some("B"),
        30,
        "2024-06-15 12:00:00",
    )
    .await;
    insert_user_with_timestamp(
        &client,
        "u3",
        "c@test.com",
        Some("C"),
        40,
        "2025-03-20 08:30:00",
    )
    .await;

    // Same SQL pattern as generated code for datetime aggregates
    let result: DateTimeAggResult = client
        .raw_fetch_one_sqlite(
            r#"SELECT MIN("created_at") as "min_created_at", MAX("created_at") as "max_created_at" FROM "users" WHERE 1=1"#,
        )
        .await
        .expect("aggregate datetime min/max");

    let min = result.min_created_at.expect("min should not be NULL");
    assert_eq!(min, "2024-01-01 10:00:00");
    let max = result.max_created_at.expect("max should not be NULL");
    assert_eq!(max, "2025-03-20 08:30:00");
}

// ═══════════════════════════════════════════════════════════════════════
// Select (Partial Loading) Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_select_specific_columns() {
    let client = setup_client().await;

    insert_user(&client, "u1", "alice@test.com", Some("Alice"), 30, true).await;

    // SELECT only id and email (same pattern as build_select_columns)
    let result: PartialUser = client
        .raw_fetch_one_sqlite(r#"SELECT "id", "email" FROM "users" WHERE 1=1 AND "id" = 'u1'"#)
        .await
        .expect("select specific columns");

    assert_eq!(result.id, Some("u1".to_string()));
    assert_eq!(result.email, Some("alice@test.com".to_string()));
    // name and age were not selected, so they should be None (sqlx default)
    assert!(
        result.name.is_none(),
        "name should be None when not selected"
    );
    assert!(result.age.is_none(), "age should be None when not selected");
}

#[tokio::test]
async fn test_select_all_columns() {
    let client = setup_client().await;

    insert_user(&client, "u1", "alice@test.com", Some("Alice"), 30, true).await;

    // SELECT all relevant columns explicitly
    let result: PartialUser = client
        .raw_fetch_one_sqlite(
            r#"SELECT "id", "email", "name", "age" FROM "users" WHERE 1=1 AND "id" = 'u1'"#,
        )
        .await
        .expect("select all columns");

    assert_eq!(result.id, Some("u1".to_string()));
    assert_eq!(result.email, Some("alice@test.com".to_string()));
    assert_eq!(result.name, Some("Alice".to_string()));
    assert_eq!(result.age, Some(30));
}

#[tokio::test]
async fn test_select_no_columns_returns_all() {
    let client = setup_client().await;

    insert_user(&client, "u1", "alice@test.com", Some("Alice"), 30, true).await;

    // When no columns are selected (all false), build_select_columns returns "*"
    let result: PartialUser = client
        .raw_fetch_one_sqlite(r#"SELECT * FROM "users" WHERE 1=1 AND "id" = 'u1'"#)
        .await
        .expect("select star");

    assert_eq!(result.id, Some("u1".to_string()));
    assert_eq!(result.email, Some("alice@test.com".to_string()));
    assert_eq!(result.name, Some("Alice".to_string()));
    assert_eq!(result.age, Some(30));
}

#[tokio::test]
async fn test_select_nullable_column() {
    let client = setup_client().await;

    // Insert user with name = NULL
    insert_user(&client, "u1", "no-name@test.com", None, 25, true).await;

    // SELECT the nullable column
    let result: PartialUser = client
        .raw_fetch_one_sqlite(r#"SELECT "id", "name" FROM "users" WHERE 1=1 AND "id" = 'u1'"#)
        .await
        .expect("select nullable column");

    assert_eq!(result.id, Some("u1".to_string()));
    assert!(
        result.name.is_none(),
        "name should be None when DB value is NULL"
    );
}

#[tokio::test]
async fn test_select_with_where_filter() {
    let client = setup_client().await;

    insert_user(&client, "u1", "a@test.com", Some("Alice"), 20, true).await;
    insert_user(&client, "u2", "b@test.com", Some("Bob"), 30, true).await;
    insert_user(&client, "u3", "c@test.com", Some("Carol"), 40, false).await;

    // SELECT specific columns with a WHERE condition (same pattern as generated code)
    let results: Vec<PartialUser> = client
        .raw_fetch_all_sqlite(
            r#"SELECT "id", "email" FROM "users" WHERE 1=1 AND "active" = 1 ORDER BY "id""#,
        )
        .await
        .expect("select with where filter");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, Some("u1".to_string()));
    assert_eq!(results[0].email, Some("a@test.com".to_string()));
    assert!(results[0].name.is_none());
    assert!(results[0].age.is_none());
    assert_eq!(results[1].id, Some("u2".to_string()));
    assert_eq!(results[1].email, Some("b@test.com".to_string()));
}
