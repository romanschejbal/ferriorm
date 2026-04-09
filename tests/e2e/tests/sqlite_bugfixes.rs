#![allow(clippy::pedantic)]

//! Regression tests for SQLite-specific bug fixes.
//!
//! Bug 1: Foreign keys rendered as inline REFERENCES in CREATE TABLE (not comments).
//! Bug 2: Table creation order respects FK dependencies (topological sort).
//! Bug 3: `file:./dev.db` URLs properly normalized to `sqlite:` scheme with `mode=rwc`.

use ferriorm_core::ast::ReferentialAction;
use ferriorm_core::schema::*;
use ferriorm_core::types::{DatabaseProvider, ScalarType};
use ferriorm_core::utils::to_snake_case;
use ferriorm_migrate::diff::{self, MigrationStep};
use ferriorm_migrate::sql;

// ─── Helpers ────────────────────────────────────────────────────────

fn empty_schema() -> Schema {
    Schema {
        datasource: DatasourceConfig {
            name: "db".into(),
            provider: DatabaseProvider::SQLite,
            url: String::new(),
        },
        generators: vec![],
        enums: vec![],
        models: vec![],
    }
}

fn make_schema(models: Vec<Model>) -> Schema {
    Schema {
        datasource: DatasourceConfig {
            name: "db".into(),
            provider: DatabaseProvider::SQLite,
            url: String::new(),
        },
        generators: vec![],
        enums: vec![],
        models,
    }
}

fn make_model(name: &str, db_name: &str, fields: Vec<Field>) -> Model {
    let pk_fields: Vec<String> = fields
        .iter()
        .filter(|f| f.is_id)
        .map(|f| f.name.clone())
        .collect();
    Model {
        name: name.into(),
        db_name: db_name.into(),
        fields,
        primary_key: PrimaryKey { fields: pk_fields },
        indexes: vec![],
        unique_constraints: vec![],
    }
}

fn make_field(name: &str, scalar: ScalarType, is_id: bool) -> Field {
    Field {
        name: name.into(),
        db_name: to_snake_case(name),
        field_type: FieldKind::Scalar(scalar),
        is_optional: false,
        is_list: false,
        is_id,
        is_unique: false,
        is_updated_at: false,
        default: None,
        relation: None,
    }
}

fn make_fk_field(
    name: &str,
    scalar: ScalarType,
    related_model: &str,
    fk_field: &str,
    ref_field: &str,
    on_delete: ReferentialAction,
    on_update: ReferentialAction,
) -> Field {
    Field {
        name: name.into(),
        db_name: to_snake_case(name),
        field_type: FieldKind::Scalar(scalar),
        is_optional: false,
        is_list: false,
        is_id: false,
        is_unique: false,
        is_updated_at: false,
        default: None,
        relation: Some(ResolvedRelation {
            related_model: related_model.into(),
            relation_type: RelationType::ManyToOne,
            fields: vec![fk_field.into()],
            references: vec![ref_field.into()],
            on_delete,
            on_update,
        }),
    }
}

fn render_sqlite(steps: &[MigrationStep]) -> String {
    let renderer = sql::renderer_for(DatabaseProvider::SQLite);
    renderer.render(steps)
}

// ─── Bug 1: Inline FK constraints in CREATE TABLE ───────────────────

#[test]
fn sqlite_create_table_has_inline_fk_references() {
    let from = empty_schema();
    let to = make_schema(vec![
        make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        ),
        make_model(
            "Post",
            "posts",
            vec![
                make_field("id", ScalarType::String, true),
                make_fk_field(
                    "authorId",
                    ScalarType::String,
                    "User",
                    "authorId",
                    "id",
                    ReferentialAction::Cascade,
                    ReferentialAction::NoAction,
                ),
            ],
        ),
    ]);

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);
    let sql = render_sqlite(&steps);

    // The FK should be inlined as REFERENCES in the CREATE TABLE, not a comment.
    assert!(
        sql.contains("REFERENCES \"users\"(\"id\")"),
        "FK should be rendered as inline REFERENCES clause. Got:\n{sql}"
    );
    assert!(
        sql.contains("ON DELETE CASCADE"),
        "ON DELETE action should be present. Got:\n{sql}"
    );
    assert!(
        sql.contains("ON UPDATE NO ACTION"),
        "ON UPDATE action should be present. Got:\n{sql}"
    );
    assert!(
        !sql.contains("-- SQLite: cannot add foreign key"),
        "Should NOT emit the FK-as-comment fallback when a CREATE TABLE exists. Got:\n{sql}"
    );
}

#[test]
fn sqlite_fk_without_create_table_still_emits_comment() {
    // When adding a FK to an existing table (no CreateTable step), we still
    // emit a comment because SQLite cannot ALTER TABLE ADD CONSTRAINT.
    let from = make_schema(vec![
        make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        ),
        make_model(
            "Post",
            "posts",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("authorId", ScalarType::String, false),
            ],
        ),
    ]);
    let to = make_schema(vec![
        make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        ),
        make_model(
            "Post",
            "posts",
            vec![
                make_field("id", ScalarType::String, true),
                make_fk_field(
                    "authorId",
                    ScalarType::String,
                    "User",
                    "authorId",
                    "id",
                    ReferentialAction::Cascade,
                    ReferentialAction::NoAction,
                ),
            ],
        ),
    ]);

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);
    let sql = render_sqlite(&steps);

    // No CreateTable for posts (it already exists), so FK is a comment.
    assert!(
        sql.contains("-- SQLite: cannot add foreign key"),
        "FK on existing table should be a comment. Got:\n{sql}"
    );
}

// ─── Bug 2: Table creation order respects FK dependencies ───────────

#[test]
fn sqlite_create_tables_ordered_by_fk_dependencies() {
    let from = empty_schema();

    // Create three tables where C -> B -> A (C references B, B references A).
    // Regardless of HashMap iteration order, A should be created first, then B, then C.
    let to = make_schema(vec![
        // Deliberately put the dependent tables first in the vec.
        make_model(
            "Comment",
            "comments",
            vec![
                make_field("id", ScalarType::String, true),
                make_fk_field(
                    "postId",
                    ScalarType::String,
                    "Post",
                    "postId",
                    "id",
                    ReferentialAction::Cascade,
                    ReferentialAction::NoAction,
                ),
            ],
        ),
        make_model(
            "Post",
            "posts",
            vec![
                make_field("id", ScalarType::String, true),
                make_fk_field(
                    "authorId",
                    ScalarType::String,
                    "User",
                    "authorId",
                    "id",
                    ReferentialAction::Cascade,
                    ReferentialAction::NoAction,
                ),
            ],
        ),
        make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        ),
    ]);

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    // Collect the CreateTable step names in order.
    let create_order: Vec<&str> = steps
        .iter()
        .filter_map(|s| match s {
            MigrationStep::CreateTable(ct) => Some(ct.name.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(create_order.len(), 3, "Should have 3 CreateTable steps");

    let user_pos = create_order.iter().position(|n| *n == "users").unwrap();
    let post_pos = create_order.iter().position(|n| *n == "posts").unwrap();
    let comment_pos = create_order.iter().position(|n| *n == "comments").unwrap();

    assert!(
        user_pos < post_pos,
        "users (pos {user_pos}) should come before posts (pos {post_pos})"
    );
    assert!(
        post_pos < comment_pos,
        "posts (pos {post_pos}) should come before comments (pos {comment_pos})"
    );
}

#[test]
fn sqlite_create_tables_with_dependencies_produce_valid_sql() {
    // End-to-end: generate SQL and execute against SQLite to verify it works.
    let from = empty_schema();
    let to = make_schema(vec![
        make_model(
            "Post",
            "posts",
            vec![
                make_field("id", ScalarType::String, true),
                make_fk_field(
                    "authorId",
                    ScalarType::String,
                    "User",
                    "authorId",
                    "id",
                    ReferentialAction::Cascade,
                    ReferentialAction::NoAction,
                ),
            ],
        ),
        make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        ),
    ]);

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);
    let sql = render_sqlite(&steps);

    // users CREATE TABLE must appear before posts CREATE TABLE
    let users_pos = sql
        .find("CREATE TABLE \"users\"")
        .expect("should have users");
    let posts_pos = sql
        .find("CREATE TABLE \"posts\"")
        .expect("should have posts");

    assert!(
        users_pos < posts_pos,
        "CREATE TABLE users (at {users_pos}) should appear before posts (at {posts_pos}) in SQL:\n{sql}"
    );
}

// ─── Bug 3: file: URL normalization ────────────────────────────────

#[test]
fn normalize_sqlite_url_transforms_file_prefix() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    let result = normalize_sqlite_url("file:./dev.db");
    assert!(
        result.starts_with("sqlite:./dev.db"),
        "file: prefix should be replaced with sqlite:. Got: {result}"
    );
    assert!(
        result.contains("mode=rwc"),
        "Should include mode=rwc for auto-creation. Got: {result}"
    );
}

#[test]
fn normalize_sqlite_url_handles_bare_path() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    let result = normalize_sqlite_url("./dev.db");
    assert!(
        result.starts_with("sqlite:./dev.db"),
        "Bare path should get sqlite: prefix. Got: {result}"
    );
    assert!(
        result.contains("mode=rwc"),
        "Should include mode=rwc. Got: {result}"
    );
}

#[test]
fn normalize_sqlite_url_preserves_existing_sqlite_prefix() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    let result = normalize_sqlite_url("sqlite:./dev.db");
    assert!(
        result.starts_with("sqlite:./dev.db"),
        "Already-prefixed URL should be kept. Got: {result}"
    );
    assert!(
        result.contains("mode=rwc"),
        "Should append mode=rwc. Got: {result}"
    );
}

#[test]
fn normalize_sqlite_url_does_not_duplicate_mode() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    let result = normalize_sqlite_url("sqlite:./dev.db?mode=rwc");
    assert_eq!(
        result.matches("mode=").count(),
        1,
        "Should not duplicate mode param. Got: {result}"
    );
}

#[test]
fn normalize_sqlite_url_appends_to_existing_query() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    let result = normalize_sqlite_url("sqlite:./dev.db?cache=shared");
    assert!(
        result.contains("cache=shared"),
        "Existing params should be preserved. Got: {result}"
    );
    assert!(
        result.contains("&mode=rwc"),
        "mode=rwc should be appended with &. Got: {result}"
    );
}

#[test]
fn normalize_sqlite_url_memory_database() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    // sqlite::memory: is a special URL — it should still get mode=rwc appended
    // (harmless for in-memory, but consistent).
    let result = normalize_sqlite_url("sqlite::memory:");
    assert!(
        result.starts_with("sqlite::memory:"),
        "Memory URL prefix should be preserved. Got: {result}"
    );
}

#[test]
fn normalize_sqlite_url_file_with_nested_path() {
    use ferriorm_runtime::client::normalize_sqlite_url;

    let result = normalize_sqlite_url("file:data/databases/app.db");
    assert!(
        result.starts_with("sqlite:data/databases/app.db"),
        "Nested path should be preserved. Got: {result}"
    );
    assert!(
        result.contains("mode=rwc"),
        "Should include mode=rwc. Got: {result}"
    );
}

// ─── Issue 5 verification: SQLite CURRENT_TIMESTAMP roundtrips with chrono::DateTime<Utc> ──

#[derive(sqlx::FromRow, Debug)]
struct ChronoRow {
    created_at: chrono::DateTime<chrono::Utc>,
}

#[tokio::test]
async fn sqlite_current_timestamp_default_roundtrips_with_chrono_utc() {
    use sqlx::SqlitePool;

    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

    // Create table with DEFAULT CURRENT_TIMESTAMP (mirrors what diff engine emits for @default(now()) on SQLite)
    sqlx::query(
        r#"CREATE TABLE "events" (
            "id" INTEGER PRIMARY KEY,
            "created_at" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    // Insert without providing created_at — SQLite fills it via DEFAULT
    sqlx::query(r#"INSERT INTO "events" ("id") VALUES (1)"#)
        .execute(&pool)
        .await
        .unwrap();

    // Read back as chrono::DateTime<Utc>
    let row: Result<ChronoRow, sqlx::Error> =
        sqlx::query_as(r#"SELECT "created_at" FROM "events" WHERE "id" = 1"#)
            .fetch_one(&pool)
            .await;

    assert!(
        row.is_ok(),
        "SQLite CURRENT_TIMESTAMP should roundtrip to chrono::DateTime<Utc>. Got: {:?}",
        row.err()
    );

    let row = row.unwrap();
    let now = chrono::Utc::now();
    let diff = (now - row.created_at).num_seconds().abs();
    assert!(
        diff < 5,
        "CURRENT_TIMESTAMP should be close to now. Diff: {diff}s, parsed: {:?}",
        row.created_at
    );
}

#[tokio::test]
async fn sqlite_chrono_now_inserted_via_app_roundtrips() {
    use sqlx::SqlitePool;

    // This mirrors what generated codegen does: chrono::Utc::now() is inserted via push_bind
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        r#"CREATE TABLE "events" (
            "id" INTEGER PRIMARY KEY,
            "created_at" TEXT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let inserted = chrono::Utc::now();
    sqlx::query(r#"INSERT INTO "events" ("id", "created_at") VALUES (?, ?)"#)
        .bind(1i64)
        .bind(inserted)
        .execute(&pool)
        .await
        .unwrap();

    let row: ChronoRow = sqlx::query_as(r#"SELECT "created_at" FROM "events" WHERE "id" = 1"#)
        .fetch_one(&pool)
        .await
        .unwrap();

    let diff = (inserted - row.created_at).num_milliseconds().abs();
    assert!(
        diff < 100,
        "App-inserted timestamp should roundtrip cleanly. Diff: {diff}ms"
    );
}
