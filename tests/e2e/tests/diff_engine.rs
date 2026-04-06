//! End-to-end tests for the schema diff engine.
//!
//! These tests verify that various schema modifications produce the correct
//! MigrationStep variants and that the rendered SQL is correct for SQLite.

use ormx_core::ast::{DefaultValue, LiteralValue, ReferentialAction};
use ormx_core::schema::*;
use ormx_core::types::{DatabaseProvider, ScalarType};
use ormx_core::utils::to_snake_case;
use ormx_migrate::diff::{self, MigrationStep};
use ormx_migrate::sql;

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

fn make_schema(models: Vec<Model>, enums: Vec<Enum>) -> Schema {
    Schema {
        datasource: DatasourceConfig {
            name: "db".into(),
            provider: DatabaseProvider::SQLite,
            url: String::new(),
        },
        generators: vec![],
        enums,
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

fn make_optional_field(name: &str, scalar: ScalarType) -> Field {
    Field {
        name: name.into(),
        db_name: to_snake_case(name),
        field_type: FieldKind::Scalar(scalar),
        is_optional: true,
        is_list: false,
        is_id: false,
        is_unique: false,
        is_updated_at: false,
        default: None,
        relation: None,
    }
}

fn make_unique_field(name: &str, scalar: ScalarType) -> Field {
    Field {
        name: name.into(),
        db_name: to_snake_case(name),
        field_type: FieldKind::Scalar(scalar),
        is_optional: false,
        is_list: false,
        is_id: false,
        is_unique: true,
        is_updated_at: false,
        default: None,
        relation: None,
    }
}

fn make_field_with_default(name: &str, scalar: ScalarType, default: DefaultValue) -> Field {
    Field {
        name: name.into(),
        db_name: to_snake_case(name),
        field_type: FieldKind::Scalar(scalar),
        is_optional: false,
        is_list: false,
        is_id: false,
        is_unique: false,
        is_updated_at: false,
        default: Some(default),
        relation: None,
    }
}

fn make_enum_field(name: &str, enum_name: &str) -> Field {
    Field {
        name: name.into(),
        db_name: to_snake_case(name),
        field_type: FieldKind::Enum(enum_name.into()),
        is_optional: false,
        is_list: false,
        is_id: false,
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
            on_delete: ReferentialAction::Cascade,
            on_update: ReferentialAction::NoAction,
        }),
    }
}

fn render_sqlite(steps: &[MigrationStep]) -> String {
    let renderer = sql::renderer_for(DatabaseProvider::SQLite);
    renderer.render(steps)
}

// ─── Tests ──────────────────────────────────────────────────────────

#[test]
fn diff_add_model() {
    let from = empty_schema();
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("email", ScalarType::String, false),
            ],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(
        matches!(&steps[0], MigrationStep::CreateTable(ct) if ct.name == "users"),
        "Should produce CreateTable step"
    );

    if let MigrationStep::CreateTable(ct) = &steps[0] {
        assert_eq!(ct.columns.len(), 2);
        assert_eq!(ct.primary_key, vec!["id"]);
        assert_eq!(ct.columns[0].name, "id");
        assert_eq!(ct.columns[0].sql_type, "TEXT");
        assert!(!ct.columns[0].nullable);
        assert_eq!(ct.columns[1].name, "email");
    }

    // Verify SQL output
    let sql = render_sqlite(&steps);
    assert!(sql.contains("CREATE TABLE \"users\""));
    assert!(sql.contains("\"id\" TEXT NOT NULL"));
    assert!(sql.contains("\"email\" TEXT NOT NULL"));
    assert!(sql.contains("PRIMARY KEY (\"id\")"));
}

#[test]
fn diff_remove_model() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        )],
        vec![],
    );
    let to = empty_schema();

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(
        matches!(&steps[0], MigrationStep::DropTable { name } if name == "users"),
        "Should produce DropTable step"
    );

    let sql = render_sqlite(&steps);
    assert!(sql.contains("DROP TABLE IF EXISTS \"users\""));
}

#[test]
fn diff_add_column() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        )],
        vec![],
    );
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_optional_field("bio", ScalarType::String),
            ],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(
        matches!(&steps[0], MigrationStep::AddColumn { table, column }
            if table == "users" && column.name == "bio" && column.nullable),
        "Should produce AddColumn step for nullable bio"
    );

    let sql = render_sqlite(&steps);
    assert!(sql.contains("ALTER TABLE \"users\" ADD COLUMN \"bio\" TEXT"));
    // Nullable column should NOT have NOT NULL
    assert!(
        !sql.contains("\"bio\" TEXT NOT NULL"),
        "Nullable column should not have NOT NULL"
    );
}

#[test]
fn diff_remove_column() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("email", ScalarType::String, false),
            ],
        )],
        vec![],
    );
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(
        matches!(&steps[0], MigrationStep::DropColumn { table, column }
            if table == "users" && column == "email"),
        "Should produce DropColumn step"
    );

    let sql = render_sqlite(&steps);
    assert!(sql.contains("ALTER TABLE \"users\" DROP COLUMN \"email\""));
}

#[test]
fn diff_add_enum() {
    let from = empty_schema();
    let to = make_schema(
        vec![],
        vec![Enum {
            name: "Role".into(),
            db_name: "role".into(),
            variants: vec!["Admin".into(), "User".into(), "Guest".into()],
        }],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(
        matches!(&steps[0], MigrationStep::CreateEnum { name, variants }
            if name == "role" && variants.len() == 3),
        "Should produce CreateEnum step"
    );

    // SQLite renders enums as comments
    let sql = render_sqlite(&steps);
    assert!(
        sql.contains("-- SQLite: enum"),
        "SQLite should emit enum as comment"
    );
    assert!(sql.contains("'admin'"), "Comment should list enum values");
}

#[test]
fn diff_drop_enum() {
    let from = make_schema(
        vec![],
        vec![Enum {
            name: "Role".into(),
            db_name: "role".into(),
            variants: vec!["Admin".into(), "User".into()],
        }],
    );
    let to = empty_schema();

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(matches!(
        &steps[0],
        MigrationStep::DropEnum { name } if name == "role"
    ));
}

#[test]
fn diff_add_enum_variant() {
    let from = make_schema(
        vec![],
        vec![Enum {
            name: "Status".into(),
            db_name: "status".into(),
            variants: vec!["Active".into(), "Inactive".into()],
        }],
    );
    let to = make_schema(
        vec![],
        vec![Enum {
            name: "Status".into(),
            db_name: "status".into(),
            variants: vec!["Active".into(), "Inactive".into(), "Suspended".into()],
        }],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    assert!(
        matches!(&steps[0], MigrationStep::AddEnumVariant { enum_name, variant }
            if enum_name == "status" && variant == "Suspended"),
        "Should produce AddEnumVariant step"
    );

    let sql = render_sqlite(&steps);
    assert!(sql.contains("suspended") || sql.contains("Suspended"));
}

#[test]
fn diff_add_index() {
    let from = make_schema(
        vec![{
            let mut m = make_model(
                "User",
                "users",
                vec![
                    make_field("id", ScalarType::String, true),
                    make_field("email", ScalarType::String, false),
                ],
            );
            m.indexes = vec![];
            m
        }],
        vec![],
    );
    let to = make_schema(
        vec![{
            let mut m = make_model(
                "User",
                "users",
                vec![
                    make_field("id", ScalarType::String, true),
                    make_field("email", ScalarType::String, false),
                ],
            );
            m.indexes = vec![Index {
                fields: vec!["email".into()],
            }];
            m
        }],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    let index_step = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::CreateIndex { .. }));
    assert!(
        index_step.is_some(),
        "Should produce CreateIndex step. Steps: {steps:?}"
    );

    if let Some(MigrationStep::CreateIndex {
        table,
        name,
        columns,
    }) = index_step
    {
        assert_eq!(table, "users");
        assert!(name.contains("email"));
        assert_eq!(columns, &vec!["email".to_string()]);
    }

    let sql = render_sqlite(&steps);
    assert!(sql.contains("CREATE INDEX"));
    assert!(sql.contains("ON \"users\""));
    assert!(sql.contains("\"email\""));
}

#[test]
fn diff_add_foreign_key() {
    let user_model = make_model(
        "User",
        "users",
        vec![make_field("id", ScalarType::String, true)],
    );

    let from = make_schema(
        vec![
            user_model.clone(),
            make_model(
                "Post",
                "posts",
                vec![
                    make_field("id", ScalarType::String, true),
                    make_field("authorId", ScalarType::String, false),
                ],
            ),
        ],
        vec![],
    );

    let to = make_schema(
        vec![
            user_model,
            make_model(
                "Post",
                "posts",
                vec![
                    make_field("id", ScalarType::String, true),
                    make_fk_field("authorId", ScalarType::String, "User", "authorId", "id"),
                ],
            ),
        ],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    let fk_step = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::AddForeignKey(_)));
    assert!(
        fk_step.is_some(),
        "Should produce AddForeignKey step. Steps: {steps:?}"
    );

    if let Some(MigrationStep::AddForeignKey(fk)) = fk_step {
        assert_eq!(fk.table, "posts");
        assert_eq!(fk.column, "author_id");
        assert_eq!(fk.referenced_table, "users");
        assert_eq!(fk.referenced_column, "id");
        assert_eq!(fk.on_delete, "CASCADE");
    }

    // SQLite renders FK additions as comments
    let sql = render_sqlite(&steps);
    assert!(
        sql.contains("cannot add foreign key"),
        "SQLite should warn about FK limitation"
    );
}

#[test]
fn diff_change_column_type() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("age", ScalarType::String, false), // was String
            ],
        )],
        vec![],
    );
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("age", ScalarType::Int, false), // now Int
            ],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    let alter_step = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::AlterColumn { .. }));
    assert!(
        alter_step.is_some(),
        "Should produce AlterColumn step for type change. Steps: {steps:?}"
    );

    if let Some(MigrationStep::AlterColumn {
        table,
        column,
        changes,
    }) = alter_step
    {
        assert_eq!(table, "users");
        assert_eq!(column, "age");
        assert_eq!(changes.sql_type, Some("INTEGER".to_string()));
    }

    // SQLite emits a comment for ALTER COLUMN
    let sql = render_sqlite(&steps);
    assert!(sql.contains("ALTER COLUMN is not supported"));
    assert!(sql.contains("type -> INTEGER"));
}

#[test]
fn diff_change_column_nullability() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("name", ScalarType::String, false), // required
            ],
        )],
        vec![],
    );
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_optional_field("name", ScalarType::String), // now optional
            ],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    let alter_step = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::AlterColumn { .. }));
    assert!(
        alter_step.is_some(),
        "Should produce AlterColumn for nullability change. Steps: {steps:?}"
    );

    if let Some(MigrationStep::AlterColumn { changes, .. }) = alter_step {
        assert_eq!(
            changes.nullable,
            Some(true),
            "Should indicate column is becoming nullable"
        );
    }

    let sql = render_sqlite(&steps);
    assert!(sql.contains("DROP NOT NULL"));
}

#[test]
fn diff_multiple_changes_at_once() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("email", ScalarType::String, false),
            ],
        )],
        vec![],
    );
    let to = make_schema(
        vec![
            make_model(
                "User",
                "users",
                vec![
                    make_field("id", ScalarType::String, true),
                    // email removed
                    // name added
                    make_optional_field("name", ScalarType::String),
                    // age added
                    make_field_with_default(
                        "age",
                        ScalarType::Int,
                        DefaultValue::Literal(LiteralValue::Int(0)),
                    ),
                ],
            ),
            // New table
            make_model(
                "Post",
                "posts",
                vec![
                    make_field("id", ScalarType::String, true),
                    make_field("title", ScalarType::String, false),
                ],
            ),
        ],
        // New enum
        vec![Enum {
            name: "Role".into(),
            db_name: "role".into(),
            variants: vec!["Admin".into(), "User".into()],
        }],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    // Verify we have multiple step types
    let has_create_enum = steps
        .iter()
        .any(|s| matches!(s, MigrationStep::CreateEnum { .. }));
    let has_create_table = steps
        .iter()
        .any(|s| matches!(s, MigrationStep::CreateTable(ct) if ct.name == "posts"));
    let has_add_column = steps
        .iter()
        .any(|s| matches!(s, MigrationStep::AddColumn { .. }));
    let has_drop_column = steps
        .iter()
        .any(|s| matches!(s, MigrationStep::DropColumn { .. }));

    assert!(has_create_enum, "Should have CreateEnum step");
    assert!(has_create_table, "Should have CreateTable step for posts");
    assert!(has_add_column, "Should have AddColumn step(s)");
    assert!(has_drop_column, "Should have DropColumn step for email");

    // Verify add column details
    let add_name = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::AddColumn { column, .. } if column.name == "name"));
    assert!(add_name.is_some(), "Should add name column");
    if let Some(MigrationStep::AddColumn { column, .. }) = add_name {
        assert!(column.nullable, "name should be nullable");
    }

    let add_age = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::AddColumn { column, .. } if column.name == "age"));
    assert!(add_age.is_some(), "Should add age column");
    if let Some(MigrationStep::AddColumn { column, .. }) = add_age {
        assert_eq!(column.sql_type, "INTEGER");
        assert_eq!(column.default, Some("0".to_string()));
    }

    // Verify drop column
    let drop_email = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::DropColumn { column, .. } if column == "email"));
    assert!(drop_email.is_some(), "Should drop email column");

    // Verify the SQL is coherent
    let sql = render_sqlite(&steps);
    assert!(sql.contains("CREATE TABLE \"posts\""));
    assert!(sql.contains("ALTER TABLE \"users\" ADD COLUMN"));
    assert!(sql.contains("ALTER TABLE \"users\" DROP COLUMN \"email\""));
    assert!(sql.contains("-- SQLite: enum"));
}

#[test]
fn diff_add_unique_column() {
    let from = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![make_field("id", ScalarType::String, true)],
        )],
        vec![],
    );
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_unique_field("email", ScalarType::String),
            ],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    let add_col = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::AddColumn { .. }));
    assert!(add_col.is_some());

    if let Some(MigrationStep::AddColumn { column, .. }) = add_col {
        assert!(column.is_unique, "email column should be marked unique");
    }

    let sql = render_sqlite(&steps);
    assert!(sql.contains("UNIQUE"));
}

#[test]
fn diff_enum_used_as_column_type() {
    let from = empty_schema();
    let to = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_enum_field("role", "Role"),
            ],
        )],
        vec![Enum {
            name: "Role".into(),
            db_name: "role".into(),
            variants: vec!["Admin".into(), "User".into()],
        }],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    // Should have CreateEnum + CreateTable
    let create_table = steps
        .iter()
        .find(|s| matches!(s, MigrationStep::CreateTable(_)));
    assert!(create_table.is_some());

    if let Some(MigrationStep::CreateTable(ct)) = create_table {
        let role_col = ct.columns.iter().find(|c| c.name == "role");
        assert!(role_col.is_some(), "Should have role column");
        // SQLite renders enum types as TEXT
        assert_eq!(
            role_col.unwrap().sql_type,
            "TEXT",
            "Enum column in SQLite should be TEXT"
        );
    }
}

#[test]
fn diff_column_with_default_value() {
    let from = empty_schema();
    let to = make_schema(
        vec![make_model(
            "Config",
            "configs",
            vec![
                make_field("id", ScalarType::String, true),
                make_field_with_default(
                    "retryCount",
                    ScalarType::Int,
                    DefaultValue::Literal(LiteralValue::Int(3)),
                ),
                make_field_with_default(
                    "enabled",
                    ScalarType::Boolean,
                    DefaultValue::Literal(LiteralValue::Bool(true)),
                ),
                make_field_with_default("createdAt", ScalarType::DateTime, DefaultValue::Now),
            ],
        )],
        vec![],
    );

    let steps = diff::diff_schemas(&from, &to, DatabaseProvider::SQLite);

    assert_eq!(steps.len(), 1);
    if let MigrationStep::CreateTable(ct) = &steps[0] {
        let retry = ct.columns.iter().find(|c| c.name == "retry_count").unwrap();
        assert_eq!(retry.default, Some("3".to_string()));

        let enabled = ct.columns.iter().find(|c| c.name == "enabled").unwrap();
        assert_eq!(enabled.default, Some("TRUE".to_string()));

        let created = ct.columns.iter().find(|c| c.name == "created_at").unwrap();
        assert_eq!(created.default, Some("CURRENT_TIMESTAMP".to_string()));
    }

    let sql = render_sqlite(&steps);
    assert!(sql.contains("DEFAULT 3"));
    assert!(sql.contains("DEFAULT TRUE"));
    assert!(sql.contains("DEFAULT CURRENT_TIMESTAMP"));
}

#[test]
fn diff_no_changes_produces_empty_steps() {
    let schema = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("email", ScalarType::String, false),
                make_optional_field("name", ScalarType::String),
            ],
        )],
        vec![Enum {
            name: "Status".into(),
            db_name: "status".into(),
            variants: vec!["Active".into(), "Inactive".into()],
        }],
    );

    let steps = diff::diff_schemas(&schema, &schema, DatabaseProvider::SQLite);
    assert!(
        steps.is_empty(),
        "Diffing identical schemas should produce no steps"
    );
}

#[test]
fn diff_snapshot_round_trip() {
    // Verify that serializing a schema to JSON and back produces the same diff result
    let schema = make_schema(
        vec![make_model(
            "User",
            "users",
            vec![
                make_field("id", ScalarType::String, true),
                make_field("email", ScalarType::String, false),
            ],
        )],
        vec![Enum {
            name: "Role".into(),
            db_name: "role".into(),
            variants: vec!["Admin".into(), "User".into()],
        }],
    );

    let json = ormx_migrate::snapshot::serialize(&schema).expect("serialize");
    let deserialized = ormx_migrate::snapshot::deserialize(&json).expect("deserialize");

    // Diff the original and deserialized -- should be empty (no changes)
    let steps = diff::diff_schemas(&schema, &deserialized, DatabaseProvider::SQLite);
    assert!(
        steps.is_empty(),
        "Roundtripped schema should produce no diff. Steps: {steps:?}"
    );

    // Verify the deserialized schema matches
    assert_eq!(deserialized.models.len(), schema.models.len());
    assert_eq!(deserialized.enums.len(), schema.enums.len());
    assert_eq!(deserialized.models[0].name, "User");
    assert_eq!(deserialized.models[0].db_name, "users");
    assert_eq!(deserialized.enums[0].name, "Role");
}

#[test]
fn diff_parsed_schemas_end_to_end() {
    // Parse two real schema strings and diff them
    let v1_source = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

model User {
  id    String @id
  email String @unique

  @@map("users")
}
"#;

    let v2_source = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

enum Role {
  Admin
  User
}

model User {
  id    String @id
  email String @unique
  name  String?
  role  Role   @default(User)

  @@map("users")
}

model Post {
  id       String @id
  title    String
  authorId String

  @@map("posts")
  @@index([authorId])
}
"#;

    let schema_v1 = ormx_parser::parse_and_validate(v1_source).expect("parse v1");
    let schema_v2 = ormx_parser::parse_and_validate(v2_source).expect("parse v2");

    let steps = diff::diff_schemas(&schema_v1, &schema_v2, DatabaseProvider::SQLite);

    // Should have:
    // - CreateEnum for Role
    // - CreateTable for posts
    // - AddColumn for name on users
    // - AddColumn for role on users
    // - CreateIndex for authorId on posts
    let has_create_enum = steps
        .iter()
        .any(|s| matches!(s, MigrationStep::CreateEnum { name, .. } if name == "role"));
    let has_create_posts = steps
        .iter()
        .any(|s| matches!(s, MigrationStep::CreateTable(ct) if ct.name == "posts"));
    let has_add_name = steps.iter().any(|s| {
        matches!(s, MigrationStep::AddColumn { table, column } if table == "users" && column.name == "name")
    });
    let has_add_role = steps.iter().any(|s| {
        matches!(s, MigrationStep::AddColumn { table, column } if table == "users" && column.name == "role")
    });

    assert!(has_create_enum, "Should create Role enum. Steps: {steps:?}");
    assert!(
        has_create_posts,
        "Should create posts table. Steps: {steps:?}"
    );
    assert!(
        has_add_name,
        "Should add name column to users. Steps: {steps:?}"
    );
    assert!(
        has_add_role,
        "Should add role column to users. Steps: {steps:?}"
    );

    // Render to SQL and verify
    let sql = render_sqlite(&steps);
    assert!(sql.contains("CREATE TABLE \"posts\""));
    assert!(sql.contains("ALTER TABLE \"users\" ADD COLUMN \"name\" TEXT"));
    assert!(sql.contains("ALTER TABLE \"users\" ADD COLUMN \"role\" TEXT"));
}
