//! End-to-end tests for parsing a schema and generating Rust code.
//!
//! These tests verify the full pipeline: parse schema string -> validate ->
//! generate Rust files -> verify output files exist and contain valid syntax.

const SCHEMA: &str = r#"
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
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  posts     Post[]

  @@map("users")
}

model Post {
  id        String   @id @default(uuid())
  title     String
  content   String?
  published Boolean  @default(false)
  authorId  String
  author    User     @relation(fields: [authorId], references: [id])
  createdAt DateTime @default(now())

  @@map("posts")
  @@index([authorId])
}
"#;

#[test]
fn parse_and_validate_schema() {
    let schema =
        ferriorm_parser::parse_and_validate(SCHEMA).expect("parse_and_validate should succeed");

    // Verify datasource
    assert_eq!(
        schema.datasource.provider,
        ferriorm_core::types::DatabaseProvider::SQLite
    );
    assert_eq!(schema.datasource.url, "sqlite::memory:");

    // Verify enums
    assert_eq!(schema.enums.len(), 1);
    assert_eq!(schema.enums[0].name, "Status");
    assert_eq!(schema.enums[0].db_name, "status");
    assert_eq!(
        schema.enums[0].variants,
        vec!["Active", "Inactive", "Suspended"]
    );

    // Verify models
    assert_eq!(schema.models.len(), 2);

    let user = schema
        .models
        .iter()
        .find(|m| m.name == "User")
        .expect("User model should exist");
    assert_eq!(user.db_name, "users");
    assert_eq!(user.primary_key.fields, vec!["id"]);

    // Check User fields
    let id_field = user
        .fields
        .iter()
        .find(|f| f.name == "id")
        .expect("id field");
    assert!(id_field.is_id);
    assert_eq!(
        id_field.field_type,
        ferriorm_core::schema::FieldKind::Scalar(ferriorm_core::types::ScalarType::String)
    );

    let email_field = user
        .fields
        .iter()
        .find(|f| f.name == "email")
        .expect("email field");
    assert!(email_field.is_unique);
    assert!(!email_field.is_optional);

    let name_field = user
        .fields
        .iter()
        .find(|f| f.name == "name")
        .expect("name field");
    assert!(name_field.is_optional);

    let status_field = user
        .fields
        .iter()
        .find(|f| f.name == "status")
        .expect("status field");
    assert_eq!(
        status_field.field_type,
        ferriorm_core::schema::FieldKind::Enum("Status".into())
    );

    let updated_at_field = user
        .fields
        .iter()
        .find(|f| f.name == "updatedAt")
        .expect("updatedAt field");
    assert!(updated_at_field.is_updated_at);

    // Verify Post model
    let post = schema
        .models
        .iter()
        .find(|m| m.name == "Post")
        .expect("Post model should exist");
    assert_eq!(post.db_name, "posts");
    assert_eq!(post.indexes.len(), 1);
    assert_eq!(post.indexes[0].fields, vec!["authorId"]);

    // Check Post relation field
    let author_field = post
        .fields
        .iter()
        .find(|f| f.name == "author")
        .expect("author field");
    assert!(author_field.relation.is_some());
    let rel = author_field.relation.as_ref().unwrap();
    assert_eq!(rel.related_model, "User");
    assert_eq!(rel.fields, vec!["authorId"]);
    assert_eq!(rel.references, vec!["id"]);
}

#[test]
fn generate_code_to_temp_dir() {
    let schema =
        ferriorm_parser::parse_and_validate(SCHEMA).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    // Verify expected files exist
    let expected_files = ["mod.rs", "client.rs", "user.rs", "post.rs", "enums.rs"];
    for filename in &expected_files {
        let path = output_dir.join(filename);
        assert!(
            path.exists(),
            "Expected file {filename} to exist at {}",
            path.display()
        );
    }

    // Verify each generated file is valid Rust syntax by parsing with syn
    for filename in &expected_files {
        let path = output_dir.join(filename);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {filename}: {e}"));
        syn::parse_file(&content)
            .unwrap_or_else(|e| panic!("Generated file {filename} has invalid Rust syntax: {e}"));
    }
}

#[test]
fn generated_code_contains_expected_structures() {
    let schema =
        ferriorm_parser::parse_and_validate(SCHEMA).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    // Verify mod.rs exports
    let mod_content = std::fs::read_to_string(output_dir.join("mod.rs")).unwrap();
    assert!(
        mod_content.contains("pub mod user;"),
        "mod.rs should export user module"
    );
    assert!(
        mod_content.contains("pub mod post;"),
        "mod.rs should export post module"
    );
    assert!(
        mod_content.contains("pub mod client;"),
        "mod.rs should export client module"
    );
    assert!(
        mod_content.contains("pub mod enums;"),
        "mod.rs should export enums module"
    );
    assert!(
        mod_content.contains("pub use client::FerriormClient;"),
        "mod.rs should re-export FerriormClient"
    );

    // Verify user.rs contains the User struct
    let user_content = std::fs::read_to_string(output_dir.join("user.rs")).unwrap();
    assert!(
        user_content.contains("struct User"),
        "user.rs should contain User struct"
    );

    // Verify post.rs contains the Post struct
    let post_content = std::fs::read_to_string(output_dir.join("post.rs")).unwrap();
    assert!(
        post_content.contains("struct Post"),
        "post.rs should contain Post struct"
    );

    // Verify enums.rs contains the Status enum
    let enums_content = std::fs::read_to_string(output_dir.join("enums.rs")).unwrap();
    assert!(
        enums_content.contains("enum Status"),
        "enums.rs should contain Status enum"
    );
    assert!(
        enums_content.contains("Active"),
        "Status enum should contain Active variant"
    );
    assert!(
        enums_content.contains("Inactive"),
        "Status enum should contain Inactive variant"
    );
    assert!(
        enums_content.contains("Suspended"),
        "Status enum should contain Suspended variant"
    );

    // Verify client.rs contains FerriormClient
    let client_content = std::fs::read_to_string(output_dir.join("client.rs")).unwrap();
    assert!(
        client_content.contains("FerriormClient"),
        "client.rs should contain FerriormClient struct"
    );
}

#[test]
fn parse_invalid_schema_returns_error() {
    let invalid = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

model User {
  email String
  name  String
}
"#;
    // Missing @id should fail validation
    let result = ferriorm_parser::parse_and_validate(invalid);
    assert!(
        result.is_err(),
        "Schema without primary key should fail validation"
    );
}

#[test]
fn parse_schema_with_unknown_type_returns_error() {
    let invalid = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

model User {
  id   String @id
  role UnknownType
}
"#;
    let result = ferriorm_parser::parse_and_validate(invalid);
    assert!(
        result.is_err(),
        "Schema with unknown type should fail validation"
    );
}

#[test]
fn generate_schema_without_enums_skips_enums_file() {
    let schema_no_enums = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

model User {
  id    String @id @default(uuid())
  email String @unique

  @@map("users")
}
"#;
    let schema = ferriorm_parser::parse_and_validate(schema_no_enums)
        .expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    assert!(
        !output_dir.join("enums.rs").exists(),
        "enums.rs should not exist when schema has no enums"
    );

    let mod_content = std::fs::read_to_string(output_dir.join("mod.rs")).unwrap();
    assert!(
        !mod_content.contains("pub mod enums;"),
        "mod.rs should not export enums module when there are no enums"
    );
}

// ─── Bug 4 regression: integer literal types for defaults ──────────

#[test]
fn generated_default_int_on_int_field_uses_i32() {
    // @default(0) on an Int field should produce 0i32, not 0i64
    let schema_str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

model Item {
  id    String @id @default(uuid())
  count Int    @default(14)

  @@map("items")
}
"#;
    let schema =
        ferriorm_parser::parse_and_validate(schema_str).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    let item_content = std::fs::read_to_string(output_dir.join("item.rs")).unwrap();
    // Should contain 14i32, not 14i64
    assert!(
        item_content.contains("14i32"),
        "Int field default should use i32 literal. Content:\n{item_content}"
    );
    assert!(
        !item_content.contains("14i64"),
        "Int field default should NOT use i64 literal. Content:\n{item_content}"
    );
}

#[test]
fn generated_default_int_on_float_field_uses_f64() {
    // @default(0) on a Float field should produce 0f64, not 0i64
    let schema_str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

model Item {
  id    String @id @default(uuid())
  score Float  @default(0)

  @@map("items")
}
"#;
    let schema =
        ferriorm_parser::parse_and_validate(schema_str).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    let item_content = std::fs::read_to_string(output_dir.join("item.rs")).unwrap();
    // Should contain 0f64 or 0.0, not 0i64
    assert!(
        item_content.contains("0f64") || item_content.contains("0.0"),
        "Float field default should use f64 literal. Content:\n{item_content}"
    );
    assert!(
        !item_content.contains("0i64"),
        "Float field default should NOT use i64 literal. Content:\n{item_content}"
    );
}

// ─── Bug 7 regression: explicit sqlx import in generated code ──────

#[test]
fn generated_model_has_explicit_sqlx_import() {
    let schema =
        ferriorm_parser::parse_and_validate(SCHEMA).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    // Model files should have explicit sqlx, chrono, uuid imports
    let user_content = std::fs::read_to_string(output_dir.join("user.rs")).unwrap();
    assert!(
        user_content.contains("use ferriorm_runtime::prelude::sqlx;"),
        "Model file should have explicit sqlx import"
    );
    assert!(
        user_content.contains("use ferriorm_runtime::prelude::chrono;"),
        "Model file should have explicit chrono import"
    );
    assert!(
        user_content.contains("use ferriorm_runtime::prelude::uuid;"),
        "Model file should have explicit uuid import"
    );

    // Enums file should have explicit sqlx import
    let enums_content = std::fs::read_to_string(output_dir.join("enums.rs")).unwrap();
    assert!(
        enums_content.contains("use ferriorm_runtime::prelude::sqlx;"),
        "Enums file should have explicit sqlx import"
    );

    // Client file should have explicit sqlx import
    let client_content = std::fs::read_to_string(output_dir.join("client.rs")).unwrap();
    assert!(
        client_content.contains("use ferriorm_runtime::prelude::sqlx;"),
        "Client file should have explicit sqlx import"
    );
}

// ─── Bug 6 regression: PoolConfig accessible in generated code ─────

#[test]
fn generated_client_uses_pool_config_from_prelude() {
    let schema =
        ferriorm_parser::parse_and_validate(SCHEMA).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    let client_content = std::fs::read_to_string(output_dir.join("client.rs")).unwrap();
    // Should use PoolConfig (not ferriorm_runtime::client::PoolConfig)
    assert!(
        client_content.contains("config: &PoolConfig"),
        "Generated client should reference PoolConfig (from prelude)"
    );
    assert!(
        !client_content.contains("ferriorm_runtime::client::PoolConfig"),
        "Generated client should NOT use full path for PoolConfig"
    );
}

// ─── Bug 5 regression: optional FK field in relation loading ───────

#[test]
fn generated_code_with_optional_fk_is_valid_syntax() {
    // When a FK field is optional, the generated code should use
    // filter_map and handle Option properly.
    let schema_str = r#"
datasource db {
  provider = "sqlite"
  url      = "sqlite::memory:"
}

generator client {
  output = "./src/generated"
}

model Invoice {
  id                   String              @id @default(uuid())
  numberingSequence    NumberingSequence?   @relation(fields: [numberingSequenceId], references: [id])
  numberingSequenceId  String?

  @@map("invoices")
}

model NumberingSequence {
  id       String    @id @default(uuid())
  name     String
  invoices Invoice[]

  @@map("numbering_sequences")
}
"#;
    let schema =
        ferriorm_parser::parse_and_validate(schema_str).expect("parse_and_validate should succeed");

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp_dir.path().join("generated");

    ferriorm_codegen::generator::generate(&schema, &output_dir)
        .expect("code generation should succeed");

    // Verify the generated file is valid Rust syntax
    let invoice_content = std::fs::read_to_string(output_dir.join("invoice.rs")).unwrap();
    syn::parse_file(&invoice_content).unwrap_or_else(|e| {
        panic!("Generated invoice.rs with optional FK should be valid Rust syntax: {e}")
    });

    // The optional FK should use filter_map instead of map
    assert!(
        invoice_content.contains("filter_map"),
        "Optional FK field should use filter_map for collecting IDs. Content:\n{invoice_content}"
    );
}
