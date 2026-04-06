//! Validated and resolved Schema IR (Intermediate Representation).
//!
//! This is the single source of truth consumed by codegen and migration.
//! It is produced by the validator from the raw AST.

use crate::ast::{DefaultValue, ReferentialAction};
use crate::types::{DatabaseProvider, ScalarType};

/// A fully validated schema.
#[derive(Debug, Clone)]
pub struct Schema {
    pub datasource: DatasourceConfig,
    pub generators: Vec<GeneratorConfig>,
    pub enums: Vec<Enum>,
    pub models: Vec<Model>,
}

/// Resolved datasource configuration.
#[derive(Debug, Clone)]
pub struct DatasourceConfig {
    pub name: String,
    pub provider: DatabaseProvider,
    pub url: String,
}

/// Resolved generator configuration.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    pub name: String,
    pub output: String,
}

/// A validated enum definition.
#[derive(Debug, Clone)]
pub struct Enum {
    pub name: String,
    pub db_name: String,
    pub variants: Vec<String>,
}

/// A validated model definition.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub db_name: String,
    pub fields: Vec<Field>,
    pub primary_key: PrimaryKey,
    pub indexes: Vec<Index>,
    pub unique_constraints: Vec<UniqueConstraint>,
}

/// A validated field definition.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub db_name: String,
    pub field_type: FieldKind,
    pub is_optional: bool,
    pub is_list: bool,
    pub is_id: bool,
    pub is_unique: bool,
    pub is_updated_at: bool,
    pub default: Option<DefaultValue>,
    pub relation: Option<ResolvedRelation>,
}

impl Field {
    /// Returns true if this field is a scalar (stored in the database), not a relation.
    pub fn is_scalar(&self) -> bool {
        self.relation.is_none() && !self.is_list
    }

    /// Returns true if this field has a server-side default and can be omitted on create.
    pub fn has_default(&self) -> bool {
        self.default.is_some() || self.is_updated_at
    }
}

/// The kind of a field's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// A scalar type (String, Int, etc.)
    Scalar(ScalarType),
    /// A reference to an enum defined in the schema.
    Enum(String),
    /// A relation to another model.
    Model(String),
}

/// A resolved relation between two models.
#[derive(Debug, Clone)]
pub struct ResolvedRelation {
    pub related_model: String,
    pub relation_type: RelationType,
    pub fields: Vec<String>,
    pub references: Vec<String>,
    pub on_delete: ReferentialAction,
    pub on_update: ReferentialAction,
}

/// The cardinality of a relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationType {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

/// Primary key definition.
#[derive(Debug, Clone)]
pub struct PrimaryKey {
    pub fields: Vec<String>,
}

impl PrimaryKey {
    pub fn is_composite(&self) -> bool {
        self.fields.len() > 1
    }
}

/// An index definition.
#[derive(Debug, Clone)]
pub struct Index {
    pub fields: Vec<String>,
}

/// A unique constraint.
#[derive(Debug, Clone)]
pub struct UniqueConstraint {
    pub fields: Vec<String>,
}
