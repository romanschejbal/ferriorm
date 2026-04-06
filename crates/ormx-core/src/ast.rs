//! Raw AST types produced by the parser.
//!
//! These represent the schema file as-written, before validation and resolution.

/// A source location span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// The top-level schema file.
#[derive(Debug, Clone)]
pub struct SchemaFile {
    pub datasource: Option<Datasource>,
    pub generators: Vec<Generator>,
    pub enums: Vec<EnumDef>,
    pub models: Vec<ModelDef>,
}

/// `datasource db { ... }`
#[derive(Debug, Clone)]
pub struct Datasource {
    pub name: String,
    pub provider: String,
    pub url: StringOrEnv,
    pub span: Span,
}

/// A string value that may reference an environment variable.
#[derive(Debug, Clone)]
pub enum StringOrEnv {
    Literal(String),
    Env(String),
}

/// `generator client { ... }`
#[derive(Debug, Clone)]
pub struct Generator {
    pub name: String,
    pub output: Option<String>,
    pub span: Span,
}

/// `enum Role { User Admin Moderator }`
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
    pub db_name: Option<String>,
    pub span: Span,
}

/// `model User { ... }`
#[derive(Debug, Clone)]
pub struct ModelDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub attributes: Vec<BlockAttribute>,
    pub span: Span,
}

/// A single field in a model.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub attributes: Vec<FieldAttribute>,
    pub span: Span,
}

/// The type of a field (e.g., `String`, `Int?`, `Post[]`).
#[derive(Debug, Clone)]
pub struct FieldType {
    pub name: String,
    pub is_list: bool,
    pub is_optional: bool,
}

/// Attributes on a field (e.g., `@id`, `@default(uuid())`).
#[derive(Debug, Clone)]
pub enum FieldAttribute {
    Id,
    Unique,
    Default(DefaultValue),
    UpdatedAt,
    Relation(RelationAttribute),
    Map(String),
    DbType(String, Vec<String>),
}

/// Default value for a field.
#[derive(Debug, Clone)]
pub enum DefaultValue {
    Uuid,
    Cuid,
    AutoIncrement,
    Now,
    Literal(LiteralValue),
    EnumVariant(String),
}

/// A literal value in the schema.
#[derive(Debug, Clone)]
pub enum LiteralValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// `@relation(fields: [...], references: [...])`
#[derive(Debug, Clone)]
pub struct RelationAttribute {
    pub name: Option<String>,
    pub fields: Vec<String>,
    pub references: Vec<String>,
    pub on_delete: Option<ReferentialAction>,
    pub on_update: Option<ReferentialAction>,
}

/// Referential actions for foreign keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferentialAction {
    Cascade,
    Restrict,
    NoAction,
    SetNull,
    SetDefault,
}

/// Block-level attributes (e.g., `@@index`, `@@unique`, `@@map`, `@@id`).
#[derive(Debug, Clone)]
pub enum BlockAttribute {
    Index(Vec<String>),
    Unique(Vec<String>),
    Map(String),
    Id(Vec<String>),
}
