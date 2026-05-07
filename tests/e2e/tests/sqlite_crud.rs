#![allow(clippy::pedantic)]

//! End-to-end CRUD tests against an in-memory SQLite database.
//!
//! Strategy:
//! 1. Parse a schema and generate migration SQL via the diff engine + SQL renderer.
//! 2. Apply the SQL to an in-memory SQLite database.
//! 3. Define matching structs with `#[derive(sqlx::FromRow)]` in the test.
//! 4. Perform CRUD operations using `sqlx::QueryBuilder` (the same mechanism the
//!    generated code uses).
//! 5. Verify data integrity with assertions on actual data.

use ferriorm_core::types::DatabaseProvider;
use ferriorm_migrate::diff;
use ferriorm_migrate::snapshot;
use ferriorm_migrate::sql;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

// ─── Test structs matching what codegen would produce ────────────────

#[derive(Debug, Clone, sqlx::FromRow, PartialEq)]
struct User {
    id: String,
    email: String,
    name: Option<String>,
    age: i64,
    active: i64, // SQLite stores booleans as INTEGER
    created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow, PartialEq)]
struct Post {
    id: String,
    title: String,
    content: Option<String>,
    author_id: String,
    published: i64,
}

// ─── Schema definition ─────────────────────────────────────────────

const TEST_SCHEMA: &str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

model User {
  id        String   @id
  email     String   @unique
  name      String?
  age       Int      @default(0)
  active    Boolean  @default(true)
  createdAt DateTime @default(now())

  @@map("users")
}

model Post {
  id        String   @id
  title     String
  content   String?
  authorId  String
  published Boolean  @default(false)
  author    User     @relation(fields: [authorId], references: [id])

  @@map("posts")
}
"#;

// ─── Helpers ────────────────────────────────────────────────────────

/// Create a fresh in-memory SQLite pool and apply migration SQL generated from
/// our schema. For the posts table we manually add the FOREIGN KEY constraint
/// because the diff engine emits foreign keys as separate `AddForeignKey` steps
/// which SQLite renders as comments (SQLite only supports FK in CREATE TABLE).
async fn setup_db() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await
        .expect("enable foreign keys");

    // Generate migration SQL from the schema using our diff engine
    let schema = ferriorm_parser::parse_and_validate(TEST_SCHEMA).expect("parse test schema");
    let empty = snapshot::empty_schema(DatabaseProvider::SQLite);
    let steps = diff::diff_schemas(&empty, &schema, DatabaseProvider::SQLite);
    let renderer = sql::renderer_for(DatabaseProvider::SQLite);
    let sql_text = renderer.render(&steps);

    // Execute each DDL statement produced by the renderer (skipping comments)
    for stmt in sql_text.split(';') {
        let trimmed = stmt.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        // Check if it is ONLY comments (multi-line comment blocks)
        let non_comment_lines: Vec<&str> = trimmed
            .lines()
            .filter(|l| !l.trim().starts_with("--") && !l.trim().is_empty())
            .collect();
        if non_comment_lines.is_empty() {
            continue;
        }
        sqlx::query(trimmed)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("Failed to execute DDL:\n{trimmed}\nError: {e}"));
    }

    // Now recreate the posts table WITH the foreign key constraint.
    // The diff engine creates the posts table without inline FK, and then emits
    // a comment about the FK limitation. We drop and recreate to get proper FK.
    sqlx::query("DROP TABLE IF EXISTS \"posts\"")
        .execute(&pool)
        .await
        .expect("drop posts for recreation");

    sqlx::query(
        r#"CREATE TABLE "posts" (
            "id" TEXT NOT NULL,
            "title" TEXT NOT NULL,
            "content" TEXT,
            "published" INTEGER NOT NULL DEFAULT FALSE,
            "author_id" TEXT NOT NULL,
            PRIMARY KEY ("id"),
            FOREIGN KEY ("author_id") REFERENCES "users"("id") ON DELETE CASCADE
        )"#,
    )
    .execute(&pool)
    .await
    .expect("recreate posts with FK");

    pool
}

/// Insert a user row using QueryBuilder (same mechanism as generated code).
async fn insert_user(
    pool: &SqlitePool,
    id: &str,
    email: &str,
    name: Option<&str>,
    age: i64,
    active: bool,
) {
    let active_int: i64 = if active { 1 } else { 0 };
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at") VALUES ("#,
    );
    qb.push_bind(id.to_string());
    qb.push(", ");
    qb.push_bind(email.to_string());
    qb.push(", ");
    qb.push_bind(name.map(|s| s.to_string()));
    qb.push(", ");
    qb.push_bind(age);
    qb.push(", ");
    qb.push_bind(active_int);
    qb.push(", datetime('now'))");

    qb.build().execute(pool).await.expect("insert user");
}

/// Insert a post row.
async fn insert_post(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    content: Option<&str>,
    author_id: &str,
    published: bool,
) {
    let published_int: i64 = if published { 1 } else { 0 };
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"INSERT INTO "posts" ("id", "title", "content", "author_id", "published") VALUES ("#,
    );
    qb.push_bind(id.to_string());
    qb.push(", ");
    qb.push_bind(title.to_string());
    qb.push(", ");
    qb.push_bind(content.map(|s| s.to_string()));
    qb.push(", ");
    qb.push_bind(author_id.to_string());
    qb.push(", ");
    qb.push_bind(published_int);
    qb.push(")");

    qb.build().execute(pool).await.expect("insert post");
}

// ─── Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_and_select() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@example.com", Some("Alice"), 30, true).await;

    let user: User = sqlx::query_as(r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#)
        .bind("u1")
        .fetch_one(&pool)
        .await
        .expect("select user");

    assert_eq!(user.id, "u1");
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.name, Some("Alice".to_string()));
    assert_eq!(user.age, 30);
    assert_eq!(user.active, 1);
    assert!(!user.created_at.is_empty(), "created_at should be set");
}

#[tokio::test]
async fn test_insert_multiple_and_select_all() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@test.com", Some("Bob"), 30, true).await;
    insert_user(&pool, "u3", "carol@test.com", Some("Carol"), 35, false).await;

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" ORDER BY "id""#,
    )
    .fetch_all(&pool)
    .await
    .expect("select all users");

    assert_eq!(users.len(), 3);
    assert_eq!(users[0].id, "u1");
    assert_eq!(users[1].id, "u2");
    assert_eq!(users[2].id, "u3");
}

#[tokio::test]
async fn test_select_with_where_equals() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@example.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@example.com", Some("Bob"), 30, true).await;

    let user: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "email" = ?"#,
    )
    .bind("bob@example.com")
    .fetch_one(&pool)
    .await
    .expect("select by email");

    assert_eq!(user.id, "u2");
    assert_eq!(user.email, "bob@example.com");
    assert_eq!(user.name, Some("Bob".to_string()));
}

#[tokio::test]
async fn test_select_with_where_like() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@example.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@other.org", Some("Bob"), 30, true).await;
    insert_user(&pool, "u3", "carol@example.com", Some("Carol"), 35, true).await;

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "email" LIKE ? ORDER BY "id""#,
    )
    .bind("%@example.com")
    .fetch_all(&pool)
    .await
    .expect("select with LIKE");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].email, "alice@example.com");
    assert_eq!(users[1].email, "carol@example.com");
}

#[tokio::test]
async fn test_select_with_where_gt_lt() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 20, true).await;
    insert_user(&pool, "u2", "bob@test.com", Some("Bob"), 25, true).await;
    insert_user(&pool, "u3", "carol@test.com", Some("Carol"), 30, true).await;
    insert_user(&pool, "u4", "dave@test.com", Some("Dave"), 35, true).await;

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "age" > ? ORDER BY "age""#,
    )
    .bind(25i64)
    .fetch_all(&pool)
    .await
    .expect("select with age > 25");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].age, 30);
    assert_eq!(users[1].age, 35);
}

#[tokio::test]
async fn test_select_with_order_by() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "carol@test.com", Some("Carol"), 30, true).await;
    insert_user(&pool, "u3", "bob@test.com", Some("Bob"), 35, true).await;

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" ORDER BY "email" DESC"#,
    )
    .fetch_all(&pool)
    .await
    .expect("select with ORDER BY DESC");

    assert_eq!(users.len(), 3);
    assert_eq!(users[0].email, "carol@test.com");
    assert_eq!(users[1].email, "bob@test.com");
    assert_eq!(users[2].email, "alice@test.com");
}

#[tokio::test]
async fn test_select_with_limit_offset() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&pool, "u2", "b@test.com", Some("B"), 25, true).await;
    insert_user(&pool, "u3", "c@test.com", Some("C"), 30, true).await;
    insert_user(&pool, "u4", "d@test.com", Some("D"), 35, true).await;
    insert_user(&pool, "u5", "e@test.com", Some("E"), 40, true).await;

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" ORDER BY "id" LIMIT ? OFFSET ?"#,
    )
    .bind(2i64)
    .bind(1i64)
    .fetch_all(&pool)
    .await
    .expect("select with LIMIT/OFFSET");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].id, "u2");
    assert_eq!(users[1].id, "u3");
}

#[tokio::test]
async fn test_update_single_field() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(r#"UPDATE "users" SET "name" = "#);
    qb.push_bind("Alicia".to_string());
    qb.push(r#" WHERE "id" = "#);
    qb.push_bind("u1".to_string());
    qb.build().execute(&pool).await.expect("update name");

    let user: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("select updated user");

    assert_eq!(user.name, Some("Alicia".to_string()));
    // Other fields unchanged
    assert_eq!(user.email, "alice@test.com");
    assert_eq!(user.age, 25);
}

#[tokio::test]
async fn test_update_multiple_fields() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(r#"UPDATE "users" SET "name" = "#);
    qb.push_bind("Alicia".to_string());
    qb.push(r#", "age" = "#);
    qb.push_bind(26i64);
    qb.push(r#" WHERE "id" = "#);
    qb.push_bind("u1".to_string());
    qb.build()
        .execute(&pool)
        .await
        .expect("update multiple fields");

    let user: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("select updated user");

    assert_eq!(user.name, Some("Alicia".to_string()));
    assert_eq!(user.age, 26);
}

#[tokio::test]
async fn test_update_set_null() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Verify name is set first
    let before: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("select before update");
    assert_eq!(before.name, Some("Alice".to_string()));

    // Set name to NULL
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(r#"UPDATE "users" SET "name" = "#);
    qb.push_bind(None::<String>);
    qb.push(r#" WHERE "id" = "#);
    qb.push_bind("u1".to_string());
    qb.build().execute(&pool).await.expect("set name to NULL");

    let after: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("select after NULL update");

    assert_eq!(after.name, None);
}

#[tokio::test]
async fn test_delete_by_id() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@test.com", Some("Bob"), 30, true).await;

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(r#"DELETE FROM "users" WHERE "id" = "#);
    qb.push_bind("u1".to_string());
    qb.build().execute(&pool).await.expect("delete by id");

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users""#,
    )
    .fetch_all(&pool)
    .await
    .expect("select remaining");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, "u2");
}

#[tokio::test]
async fn test_delete_with_filter() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@test.com", Some("Bob"), 30, false).await;
    insert_user(&pool, "u3", "carol@test.com", Some("Carol"), 35, true).await;

    // Delete inactive users
    let mut qb =
        sqlx::QueryBuilder::<sqlx::Sqlite>::new(r#"DELETE FROM "users" WHERE "active" = "#);
    qb.push_bind(0i64);
    qb.build().execute(&pool).await.expect("delete inactive");

    let users: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" ORDER BY "id""#,
    )
    .fetch_all(&pool)
    .await
    .expect("select remaining");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].id, "u1");
    assert_eq!(users[1].id, "u3");
    // All remaining should be active
    assert!(users.iter().all(|u| u.active == 1));
}

#[tokio::test]
async fn test_count() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&pool, "u2", "b@test.com", Some("B"), 25, true).await;
    insert_user(&pool, "u3", "c@test.com", Some("C"), 30, true).await;

    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .expect("count");

    assert_eq!(count.0, 3);
}

#[tokio::test]
async fn test_count_with_filter() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&pool, "u2", "b@test.com", Some("B"), 25, false).await;
    insert_user(&pool, "u3", "c@test.com", Some("C"), 30, true).await;
    insert_user(&pool, "u4", "d@test.com", Some("D"), 35, false).await;

    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users" WHERE "active" = ?"#)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .expect("count active");

    assert_eq!(count.0, 2);
}

#[tokio::test]
async fn test_unique_constraint() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Try inserting another user with the same email
    let result = sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at") VALUES (?, ?, ?, ?, ?, datetime('now'))"#,
    )
    .bind("u2")
    .bind("alice@test.com") // duplicate email
    .bind("Another Alice")
    .bind(30i64)
    .bind(1i64)
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "Inserting duplicate email should fail due to UNIQUE constraint"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("UNIQUE") || err.contains("unique") || err.contains("constraint"),
        "Error should mention UNIQUE constraint, got: {err}"
    );
}

#[tokio::test]
async fn test_nullable_fields() {
    let pool = setup_db().await;
    // Insert user with name = NULL
    insert_user(&pool, "u1", "alice@test.com", None, 25, true).await;

    let user: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("select user with null name");

    assert_eq!(user.name, None);
    // Non-nullable fields should still be set
    assert_eq!(user.id, "u1");
    assert_eq!(user.email, "alice@test.com");
}

#[tokio::test]
async fn test_foreign_key_insert() {
    let pool = setup_db().await;

    // Insert a user first
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Insert a post referencing the user
    insert_post(
        &pool,
        "p1",
        "Hello World",
        Some("My first post"),
        "u1",
        true,
    )
    .await;

    let post: Post = sqlx::query_as(
        r#"SELECT "id", "title", "content", "author_id", "published" FROM "posts" WHERE "id" = ?"#,
    )
    .bind("p1")
    .fetch_one(&pool)
    .await
    .expect("select post");

    assert_eq!(post.id, "p1");
    assert_eq!(post.title, "Hello World");
    assert_eq!(post.content, Some("My first post".to_string()));
    assert_eq!(post.author_id, "u1");
    assert_eq!(post.published, 1);

    // Verify FK constraint: try inserting a post with a non-existent author
    let bad_result = sqlx::query(
        r#"INSERT INTO "posts" ("id", "title", "content", "author_id", "published") VALUES (?, ?, ?, ?, ?)"#,
    )
    .bind("p2")
    .bind("Orphan Post")
    .bind(None::<String>)
    .bind("nonexistent_user")
    .bind(0i64)
    .execute(&pool)
    .await;

    assert!(
        bad_result.is_err(),
        "Inserting post with non-existent author_id should fail due to FK constraint"
    );
}

#[tokio::test]
async fn test_returning_star() {
    let pool = setup_db().await;

    // SQLite supports RETURNING since version 3.35.0
    let row: User = sqlx::query_as(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           RETURNING "id", "email", "name", "age", "active", "created_at""#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alice")
    .bind(30i64)
    .bind(1i64)
    .fetch_one(&pool)
    .await
    .expect("insert returning");

    assert_eq!(row.id, "u1");
    assert_eq!(row.email, "alice@test.com");
    assert_eq!(row.name, Some("Alice".to_string()));
    assert_eq!(row.age, 30);
    assert_eq!(row.active, 1);
}

#[tokio::test]
async fn test_update_returning() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    let updated: User = sqlx::query_as(
        r#"UPDATE "users" SET "name" = ?, "age" = ?
           WHERE "id" = ?
           RETURNING "id", "email", "name", "age", "active", "created_at""#,
    )
    .bind("Alicia")
    .bind(26i64)
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("update returning");

    assert_eq!(updated.id, "u1");
    assert_eq!(updated.name, Some("Alicia".to_string()));
    assert_eq!(updated.age, 26);
    // Unchanged fields preserved
    assert_eq!(updated.email, "alice@test.com");
    assert_eq!(updated.active, 1);
}

#[tokio::test]
async fn test_delete_returning() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    let deleted: User = sqlx::query_as(
        r#"DELETE FROM "users" WHERE "id" = ?
           RETURNING "id", "email", "name", "age", "active", "created_at""#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("delete returning");

    assert_eq!(deleted.id, "u1");
    assert_eq!(deleted.email, "alice@test.com");

    // Verify the row is actually gone
    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .expect("count after delete");
    assert_eq!(count.0, 0);
}

#[tokio::test]
async fn test_transaction_commit() {
    let pool = setup_db().await;

    // Begin a transaction, insert, commit
    let mut tx = pool.begin().await.expect("begin transaction");

    sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at") VALUES (?, ?, ?, ?, ?, datetime('now'))"#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alice")
    .bind(25i64)
    .bind(1i64)
    .execute(&mut *tx)
    .await
    .expect("insert in tx");

    tx.commit().await.expect("commit");

    // Verify data is persisted after commit
    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .expect("count after commit");
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn test_transaction_rollback() {
    let pool = setup_db().await;

    // Insert one user outside transaction
    insert_user(&pool, "u0", "existing@test.com", Some("Existing"), 20, true).await;

    // Begin a transaction, insert, rollback
    let mut tx = pool.begin().await.expect("begin transaction");

    sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at") VALUES (?, ?, ?, ?, ?, datetime('now'))"#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alice")
    .bind(25i64)
    .bind(1i64)
    .execute(&mut *tx)
    .await
    .expect("insert in tx");

    tx.rollback().await.expect("rollback");

    // Verify the inserted row is NOT persisted
    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .expect("count after rollback");
    assert_eq!(count.0, 1, "Only the pre-existing row should remain");

    let user: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users""#,
    )
    .fetch_one(&pool)
    .await
    .expect("select after rollback");
    assert_eq!(user.id, "u0", "Only the pre-existing user should remain");
}

#[tokio::test]
async fn test_complex_where_and() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&pool, "u2", "b@test.com", Some("B"), 25, false).await;
    insert_user(&pool, "u3", "c@test.com", Some("C"), 30, true).await;
    insert_user(&pool, "u4", "d@test.com", Some("D"), 35, false).await;
    insert_user(&pool, "u5", "e@test.com", Some("E"), 40, true).await;

    // SELECT WHERE age > 20 AND active = true
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "age" > "#,
    );
    qb.push_bind(20i64);
    qb.push(r#" AND "active" = "#);
    qb.push_bind(1i64);
    qb.push(r#" ORDER BY "age""#);

    let users: Vec<User> = qb
        .build_query_as()
        .fetch_all(&pool)
        .await
        .expect("select with AND");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].id, "u3"); // age 30, active
    assert_eq!(users[1].id, "u5"); // age 40, active
}

#[tokio::test]
async fn test_complex_where_or() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@test.com", Some("A"), 20, true).await;
    insert_user(&pool, "u2", "b@test.com", Some("B"), 25, false).await;
    insert_user(&pool, "u3", "c@test.com", Some("C"), 50, false).await;

    // SELECT WHERE age < 22 OR age > 40
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "age" < "#,
    );
    qb.push_bind(22i64);
    qb.push(r#" OR "age" > "#);
    qb.push_bind(40i64);
    qb.push(r#" ORDER BY "age""#);

    let users: Vec<User> = qb
        .build_query_as()
        .fetch_all(&pool)
        .await
        .expect("select with OR");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].id, "u1"); // age 20
    assert_eq!(users[1].id, "u3"); // age 50
}

#[tokio::test]
async fn test_batch_insert() {
    let pool = setup_db().await;

    // Build a batch INSERT using QueryBuilder's push_values
    let users_data = [
        ("u1", "a@test.com", Some("Alice"), 20i64, 1i64),
        ("u2", "b@test.com", Some("Bob"), 25, 1),
        ("u3", "c@test.com", None::<&str>, 30, 0),
        ("u4", "d@test.com", Some("Dave"), 35, 1),
        ("u5", "e@test.com", Some("Eve"), 40, 0),
    ];

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at") "#,
    );

    qb.push_values(users_data.iter(), |mut b, user| {
        b.push_bind(user.0.to_string());
        b.push_bind(user.1.to_string());
        b.push_bind(user.2.map(|s| s.to_string()));
        b.push_bind(user.3);
        b.push_bind(user.4);
        b.push("datetime('now')");
    });

    qb.build().execute(&pool).await.expect("batch insert");

    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .expect("count after batch insert");
    assert_eq!(count.0, 5);

    // Verify specific records
    let user3: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u3")
    .fetch_one(&pool)
    .await
    .expect("select user3");

    assert_eq!(user3.name, None);
    assert_eq!(user3.age, 30);
    assert_eq!(user3.active, 0);

    let user5: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u5")
    .fetch_one(&pool)
    .await
    .expect("select user5");

    assert_eq!(user5.email, "e@test.com");
    assert_eq!(user5.name, Some("Eve".to_string()));
}

// ─── Migration SQL integration tests ───────────────────────────────
//
// These verify that the SQL generated by our migration engine actually
// produces a usable database schema.

#[tokio::test]
async fn test_migration_sql_creates_correct_column_types() {
    let pool = setup_db().await;

    // Inspect the table schema via pragma
    let columns: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT name, type, \"notnull\" FROM pragma_table_info('users') ORDER BY cid",
    )
    .fetch_all(&pool)
    .await
    .expect("pragma_table_info");

    // Build a map for easier assertions
    let col_map: std::collections::HashMap<String, (String, i64)> = columns
        .into_iter()
        .map(|(name, typ, notnull)| (name, (typ, notnull)))
        .collect();

    // id: TEXT NOT NULL
    assert_eq!(col_map["id"].0, "TEXT");
    assert_eq!(col_map["id"].1, 1, "id should be NOT NULL");

    // email: TEXT NOT NULL
    assert_eq!(col_map["email"].0, "TEXT");
    assert_eq!(col_map["email"].1, 1, "email should be NOT NULL");

    // name: TEXT (nullable)
    assert_eq!(col_map["name"].0, "TEXT");
    assert_eq!(col_map["name"].1, 0, "name should be nullable");

    // age: INTEGER NOT NULL
    assert_eq!(col_map["age"].0, "INTEGER");
    assert_eq!(col_map["age"].1, 1, "age should be NOT NULL");

    // active: INTEGER NOT NULL (boolean)
    assert_eq!(col_map["active"].0, "INTEGER");
    assert_eq!(col_map["active"].1, 1, "active should be NOT NULL");

    // created_at: TEXT NOT NULL (datetime)
    assert_eq!(col_map["created_at"].0, "TEXT");
    assert_eq!(col_map["created_at"].1, 1, "created_at should be NOT NULL");
}

#[tokio::test]
async fn test_migration_sql_creates_unique_index() {
    let pool = setup_db().await;

    // Check that email has a UNIQUE index
    let _indexes: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'users' AND sql LIKE '%UNIQUE%'",
    )
    .fetch_all(&pool)
    .await
    .expect("query indexes");

    // The email column should produce either an inline UNIQUE or a UNIQUE INDEX.
    // Verify the constraint works by trying a duplicate.
    insert_user(&pool, "u1", "test@example.com", None, 25, true).await;
    let dup = sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "age", "active", "created_at") VALUES (?, ?, ?, ?, datetime('now'))"#,
    )
    .bind("u2")
    .bind("test@example.com")
    .bind(30i64)
    .bind(1i64)
    .execute(&pool)
    .await;

    assert!(
        dup.is_err(),
        "UNIQUE constraint on email should prevent duplicates"
    );
}

#[tokio::test]
async fn test_join_users_and_posts() {
    let pool = setup_db().await;

    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@test.com", Some("Bob"), 30, true).await;

    insert_post(&pool, "p1", "Alice Post 1", Some("Content 1"), "u1", true).await;
    insert_post(&pool, "p2", "Alice Post 2", None, "u1", false).await;
    insert_post(&pool, "p3", "Bob Post 1", Some("Content 3"), "u2", true).await;

    // JOIN to get posts with author names
    let rows: Vec<SqliteRow> = sqlx::query(
        r#"SELECT p."id", p."title", u."name" as author_name
           FROM "posts" p
           INNER JOIN "users" u ON p."author_id" = u."id"
           WHERE p."published" = ?
           ORDER BY p."id""#,
    )
    .bind(1i64)
    .fetch_all(&pool)
    .await
    .expect("join query");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<String, _>("id"), "p1");
    assert_eq!(rows[0].get::<String, _>("author_name"), "Alice");
    assert_eq!(rows[1].get::<String, _>("id"), "p3");
    assert_eq!(rows[1].get::<String, _>("author_name"), "Bob");
}

#[tokio::test]
async fn test_cascade_delete() {
    let pool = setup_db().await;

    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;
    insert_post(&pool, "p1", "Post 1", Some("Content"), "u1", true).await;
    insert_post(&pool, "p2", "Post 2", None, "u1", false).await;

    // Verify posts exist
    let post_count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "posts""#)
        .fetch_one(&pool)
        .await
        .expect("count posts before");
    assert_eq!(post_count.0, 2);

    // Delete the user -- should cascade to posts
    sqlx::query(r#"DELETE FROM "users" WHERE "id" = ?"#)
        .bind("u1")
        .execute(&pool)
        .await
        .expect("delete user");

    // Posts should also be deleted due to CASCADE
    let post_count_after: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "posts""#)
        .fetch_one(&pool)
        .await
        .expect("count posts after cascade");
    assert_eq!(
        post_count_after.0, 0,
        "Posts should be deleted when parent user is deleted (CASCADE)"
    );
}

#[tokio::test]
async fn test_query_builder_select_with_dynamic_filters() {
    let pool = setup_db().await;

    insert_user(&pool, "u1", "alice@example.com", Some("Alice"), 25, true).await;
    insert_user(&pool, "u2", "bob@example.com", Some("Bob"), 30, false).await;
    insert_user(&pool, "u3", "carol@other.com", Some("Carol"), 35, true).await;

    // Build a dynamic query with QueryBuilder, similar to how generated code works
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE 1=1"#,
    );

    // Dynamically add filters
    let email_filter = Some("%@example.com");
    let min_age: Option<i64> = Some(26);
    let active_filter: Option<i64> = None; // not filtering by active

    if let Some(email) = email_filter {
        qb.push(r#" AND "email" LIKE "#);
        qb.push_bind(email.to_string());
    }

    if let Some(min) = min_age {
        qb.push(r#" AND "age" >= "#);
        qb.push_bind(min);
    }

    if let Some(active) = active_filter {
        qb.push(r#" AND "active" = "#);
        qb.push_bind(active);
    }

    qb.push(r#" ORDER BY "id""#);

    let users: Vec<User> = qb
        .build_query_as()
        .fetch_all(&pool)
        .await
        .expect("dynamic query");

    // email LIKE %@example.com AND age >= 26 => only Bob (age 30)
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, "u2");
    assert_eq!(users[0].email, "bob@example.com");
}

#[tokio::test]
async fn test_upsert_on_conflict() {
    let pool = setup_db().await;

    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Attempt upsert: INSERT OR REPLACE (SQLite-specific)
    sqlx::query(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           ON CONFLICT("id") DO UPDATE SET
             "name" = excluded."name",
             "age" = excluded."age""#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alicia Updated")
    .bind(26i64)
    .bind(1i64)
    .execute(&pool)
    .await
    .expect("upsert");

    let user: User = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "id" = ?"#,
    )
    .bind("u1")
    .fetch_one(&pool)
    .await
    .expect("select after upsert");

    assert_eq!(user.name, Some("Alicia Updated".to_string()));
    assert_eq!(user.age, 26);

    // Should still be only 1 row
    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn test_upsert_insert_new_row_with_returning() {
    let pool = setup_db().await;

    // Upsert a row that doesn't exist — should INSERT
    let user: User = sqlx::query_as(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           ON CONFLICT ("id") DO UPDATE SET
             "name" = excluded."name",
             "age" = excluded."age"
           RETURNING *"#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alice")
    .bind(25i64)
    .bind(1i64)
    .fetch_one(&pool)
    .await
    .expect("upsert insert");

    assert_eq!(user.id, "u1");
    assert_eq!(user.email, "alice@test.com");
    assert_eq!(user.name, Some("Alice".to_string()));
    assert_eq!(user.age, 25);
}

#[tokio::test]
async fn test_upsert_update_existing_row_with_returning() {
    let pool = setup_db().await;

    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Upsert the same ID — should UPDATE and return updated row
    let user: User = sqlx::query_as(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           ON CONFLICT ("id") DO UPDATE SET
             "name" = excluded."name",
             "age" = excluded."age"
           RETURNING *"#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alice Updated")
    .bind(30i64)
    .bind(1i64)
    .fetch_one(&pool)
    .await
    .expect("upsert update");

    assert_eq!(user.id, "u1");
    assert_eq!(user.name, Some("Alice Updated".to_string()));
    assert_eq!(user.age, 30);

    // Still only 1 row
    let count: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM "users""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn test_upsert_noop_update_returns_existing() {
    let pool = setup_db().await;

    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Upsert with no-op update (SET id = id) — should return existing row unchanged
    let user: User = sqlx::query_as(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           ON CONFLICT ("id") DO UPDATE SET
             "id" = "id"
           RETURNING *"#,
    )
    .bind("u1")
    .bind("alice@test.com")
    .bind("Alice")
    .bind(25i64)
    .bind(1i64)
    .fetch_one(&pool)
    .await
    .expect("upsert noop");

    assert_eq!(user.id, "u1");
    assert_eq!(user.name, Some("Alice".to_string()));
    assert_eq!(user.age, 25);
}

#[tokio::test]
async fn test_upsert_on_unique_column() {
    let pool = setup_db().await;

    // Create unique index on email
    sqlx::query(r#"CREATE UNIQUE INDEX "idx_users_email" ON "users" ("email")"#)
        .execute(&pool)
        .await
        .unwrap();

    insert_user(&pool, "u1", "alice@test.com", Some("Alice"), 25, true).await;

    // Upsert conflicting on email
    let user: User = sqlx::query_as(
        r#"INSERT INTO "users" ("id", "email", "name", "age", "active", "created_at")
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           ON CONFLICT ("email") DO UPDATE SET
             "name" = excluded."name"
           RETURNING *"#,
    )
    .bind("u2")
    .bind("alice@test.com")
    .bind("Alicia")
    .bind(25i64)
    .bind(1i64)
    .fetch_one(&pool)
    .await
    .expect("upsert on email");

    assert_eq!(user.id, "u1");
    assert_eq!(user.name, Some("Alicia".to_string()));
}

// ─── D1-D5: filter parameterization stress tests ────────────────────
//
// These tests probe SQL-injection / LIKE-escape concerns and edge-case
// pagination/IN behaviors. They mirror the SQL the generated code would
// produce (see `crates/ferriorm-codegen/src/model.rs:329-340` for the
// `format!("%{}%", v)` pattern used by `contains/starts_with/ends_with`).

/// D1: `contains: Some("100%_safe")` must match the literal string
/// `100%_safe` and NOT the unrelated string `100Xsafe`. Mirrors the
/// fixed codegen path: `like_escape(v)` -> wrap with `%`s -> bind with
/// `LIKE ? ESCAPE '\'`.
#[tokio::test]
async fn d1_string_contains_with_percent_underscore_literals() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "literal@x.com", Some("100%_safe"), 1, true).await;
    insert_user(&pool, "u2", "false@x.com", Some("100Xsafe"), 2, true).await;
    insert_user(&pool, "u3", "unrelated@x.com", Some("nope"), 3, true).await;

    // Mirror fixed codegen path.
    let user_input = "100%_safe";
    let pattern = format!("%{}%", ferriorm_runtime::filter::like_escape(user_input));

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "name" LIKE "#,
    );
    qb.push_bind(pattern);
    qb.push(r" ESCAPE '\'");
    qb.push(r#" ORDER BY "id""#);

    let rows: Vec<User> = qb
        .build_query_as()
        .fetch_all(&pool)
        .await
        .expect("select with LIKE");

    let ids: Vec<&str> = rows.iter().map(|u| u.id.as_str()).collect();

    assert!(
        ids.contains(&"u1"),
        "literal `100%_safe` must match itself; got: {ids:?}"
    );
    assert!(
        !ids.contains(&"u2"),
        "`100Xsafe` must NOT match the literal `100%_safe`; got: {ids:?}"
    );
}

/// D2: contains with `'` and `\` — verifies that bind parameters are
/// not concatenated. Single-quote / backslash must be passed safely as
/// query parameters and round-trip through the DB.
#[tokio::test]
async fn d2_string_contains_with_single_quote_and_backslash() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "obrien@x.com", Some("O'Brien"), 1, true).await;
    insert_user(&pool, "u2", "back@x.com", Some(r"back\slash"), 2, true).await;
    insert_user(&pool, "u3", "plain@x.com", Some("Smith"), 3, true).await;

    for needle in ["O'Brien", r"back\slash"] {
        let pattern = format!("%{needle}%");
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" WHERE "name" LIKE "#,
        );
        qb.push_bind(pattern);

        let rows: Vec<User> = qb
            .build_query_as()
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| panic!("LIKE bind for {needle:?} must succeed: {e}"));
        assert_eq!(
            rows.len(),
            1,
            "exactly one match for {needle:?}; got: {:?}",
            rows.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
        assert_eq!(rows[0].name.as_deref(), Some(needle));
    }
}

/// D3: `equals: Some(None)` on a nullable string filter must produce
/// `IS NULL`, not `= NULL` (which is always false in SQL). Regression
/// for `219b2f0` "nullable filters".
#[tokio::test]
async fn d3_nullable_string_filter_some_none_is_null() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@x.com", Some("Alice"), 1, true).await;
    insert_user(&pool, "u2", "b@x.com", None, 2, true).await;
    insert_user(&pool, "u3", "c@x.com", Some("Carol"), 3, true).await;

    // The wrong SQL: `WHERE "name" = NULL` returns ZERO rows.
    let none_eq_count: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "users" WHERE "name" = NULL"#)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(none_eq_count, 0, "sanity: `= NULL` is always false in SQL");

    // The right SQL: `IS NULL` finds the one nullable row.
    let is_null_count: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "users" WHERE "name" IS NULL"#)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        is_null_count, 1,
        "`IS NULL` must find the row with name=None"
    );
}

/// D4: `IntFilter.in: Some(vec![])` must yield zero rows without a
/// runtime error, regardless of whether the codegen emits `IN ()`,
/// `WHERE 1 = 0`, or short-circuits the query. This pins the contract:
/// "empty IN list -> empty result, no error" — Postgres rejects the
/// naive `IN ()` form, so codegen needs to be deliberate.
#[tokio::test]
async fn d4_int_in_with_empty_vec_is_empty_set() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@x.com", Some("Alice"), 1, true).await;
    insert_user(&pool, "u2", "b@x.com", Some("Bob"), 2, true).await;

    // The tautological no-rows form is portable and guaranteed empty.
    let rows: Vec<i64> = sqlx::query_scalar(r#"SELECT "age" FROM "users" WHERE 1 = 0"#)
        .fetch_all(&pool)
        .await
        .expect("portable empty-set form must execute");
    assert!(rows.is_empty(), "empty IN must yield no rows");

    // SQLite happens to accept `IN ()` as well-formed and zero-matching.
    // Postgres does NOT — so codegen must NOT emit `IN ()` directly on
    // PG. The test documents that SQLite is lenient; the cross-DB
    // contract has to be enforced in codegen, not the DB.
    let lenient: Vec<i64> = sqlx::query_scalar(r#"SELECT "age" FROM "users" WHERE "age" IN ()"#)
        .fetch_all(&pool)
        .await
        .expect("SQLite accepts `IN ()` and returns zero rows");
    assert!(lenient.is_empty());
}

/// D4b: exercise the IN / NOT IN fragments that codegen now emits, using
/// the same `sqlx::QueryBuilder` mechanism the generated `build_where`
/// uses. Mirrors the `gen_in_arms_lhs` output:
///
/// - non-empty `r#in`  -> `AND "col" IN (?, ?, ...)`
/// - empty     `r#in`  -> `AND 1 = 0`            (portable empty-set)
/// - non-empty `not_in` -> `AND "col" NOT IN (?, ?, ...)`
/// - empty     `not_in` -> nothing emitted       (vacuously true)
#[tokio::test]
async fn d4b_int_in_and_not_in_via_query_builder() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@x.com", Some("A"), 1, true).await;
    insert_user(&pool, "u2", "b@x.com", Some("B"), 2, true).await;
    insert_user(&pool, "u3", "c@x.com", Some("C"), 3, true).await;

    // ── Helper: build "SELECT id FROM users WHERE 1=1 ..." then execute.
    async fn ids_for(
        pool: &SqlitePool,
        push: impl FnOnce(&mut sqlx::QueryBuilder<'_, sqlx::Sqlite>),
    ) -> Vec<String> {
        let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> =
            sqlx::QueryBuilder::new(r#"SELECT "id" FROM "users" WHERE 1=1"#);
        push(&mut qb);
        qb.push(r#" ORDER BY "id""#);
        qb.build_query_scalar::<String>()
            .fetch_all(pool)
            .await
            .expect("query")
    }

    // Non-empty IN: ages [1, 3] -> rows u1, u3.
    let ids = ids_for(&pool, |qb| {
        let values = vec![1_i64, 3_i64];
        qb.push(r#" AND "age" IN ("#);
        {
            let mut sep = qb.separated(", ");
            for v in &values {
                sep.push_bind(*v);
            }
        }
        qb.push(")");
    })
    .await;
    assert_eq!(ids, vec!["u1".to_string(), "u3".to_string()]);

    // Empty IN -> `1 = 0` -> zero rows. This is the Postgres-portable form.
    let ids = ids_for(&pool, |qb| {
        let values: Vec<i64> = Vec::new();
        if values.is_empty() {
            qb.push(" AND 1 = 0");
        } else {
            qb.push(r#" AND "age" IN ("#);
            {
                let mut sep = qb.separated(", ");
                for v in &values {
                    sep.push_bind(*v);
                }
            }
            qb.push(")");
        }
    })
    .await;
    assert!(
        ids.is_empty(),
        "empty IN must yield zero rows via the `1 = 0` form"
    );

    // Non-empty NOT IN: exclude ages [1] -> rows u2, u3.
    let ids = ids_for(&pool, |qb| {
        let values = vec![1_i64];
        qb.push(r#" AND "age" NOT IN ("#);
        {
            let mut sep = qb.separated(", ");
            for v in &values {
                sep.push_bind(*v);
            }
        }
        qb.push(")");
    })
    .await;
    assert_eq!(ids, vec!["u2".to_string(), "u3".to_string()]);

    // Empty NOT IN: no fragment emitted -> all rows pass.
    let ids = ids_for(&pool, |qb| {
        let values: Vec<i64> = Vec::new();
        if !values.is_empty() {
            qb.push(r#" AND "age" NOT IN ("#);
            {
                let mut sep = qb.separated(", ");
                for v in &values {
                    sep.push_bind(*v);
                }
            }
            qb.push(")");
        }
    })
    .await;
    assert_eq!(
        ids,
        vec!["u1".to_string(), "u2".to_string(), "u3".to_string()],
        "empty NOT IN must be vacuously true"
    );

    // Sanity: IN over a nullable column ("name") does NOT match NULL rows,
    // matching standard SQL semantics. This is the contract callers should
    // expect; the IS NULL branch is composed via `equals: Some(None)`.
    insert_user(&pool, "u4", "d@x.com", None, 4, true).await;
    let ids = ids_for(&pool, |qb| {
        let values = vec!["A".to_string(), "B".to_string()];
        qb.push(r#" AND "name" IN ("#);
        {
            let mut sep = qb.separated(", ");
            for v in &values {
                sep.push_bind(v.clone());
            }
        }
        qb.push(")");
    })
    .await;
    assert_eq!(
        ids,
        vec!["u1".to_string(), "u2".to_string()],
        "IN over a nullable column must skip NULL rows"
    );
}

/// D5: pagination with `LIMIT 0` must succeed and return empty;
/// `OFFSET 1_000_000` past end of table must succeed and return empty.
#[tokio::test]
async fn d5_pagination_limit_zero_and_huge_offset() {
    let pool = setup_db().await;
    insert_user(&pool, "u1", "a@x.com", Some("A"), 1, true).await;
    insert_user(&pool, "u2", "b@x.com", Some("B"), 2, true).await;

    let zero_limit: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" LIMIT 0"#,
    )
    .fetch_all(&pool)
    .await
    .expect("LIMIT 0 must execute");
    assert!(zero_limit.is_empty(), "LIMIT 0 returns no rows");

    let huge_offset: Vec<User> = sqlx::query_as(
        r#"SELECT "id", "email", "name", "age", "active", "created_at" FROM "users" LIMIT 100 OFFSET 1000000"#,
    )
    .fetch_all(&pool)
    .await
    .expect("huge OFFSET must execute");
    assert!(huge_offset.is_empty(), "OFFSET past end returns no rows");
}
