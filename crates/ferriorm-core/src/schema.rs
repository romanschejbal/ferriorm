//! Validated and resolved Schema IR (Intermediate Representation).
//!
//! This is the **single source of truth** consumed by codegen and the migration
//! engine. It is produced by the validator from the raw [`crate::ast`] AST.
//!
//! Key differences from the raw AST:
//! - Type names are resolved to [`FieldKind::Scalar`], [`FieldKind::Enum`],
//!   or [`FieldKind::Model`].
//! - Table and column names are inferred (`snake_case` + plural) or taken from
//!   `@@map` / `@map` attributes.
//! - Relations are fully resolved with cardinality and referential actions.
//! - Primary keys, indexes, and unique constraints are normalized.
//!
//! All types in this module support optional `serde` serialization (behind the
//! `serde` feature flag) for JSON schema snapshots used by the migration engine.

use crate::ast::{DefaultValue, ReferentialAction};
use crate::types::{DatabaseProvider, ScalarType};

macro_rules! serde_derive {
    ($(#[$meta:meta])* $vis:vis struct $name:ident { $($body:tt)* }) => {
        $(#[$meta])*
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        $vis struct $name { $($body)* }
    };
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($body:tt)* }) => {
        $(#[$meta])*
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        $vis enum $name { $($body)* }
    };
}

serde_derive! {
    /// A fully validated schema.
    #[derive(Debug, Clone)]
    pub struct Schema {
        pub datasource: DatasourceConfig,
        pub generators: Vec<GeneratorConfig>,
        pub enums: Vec<Enum>,
        pub models: Vec<Model>,
    }
}

serde_derive! {
    /// Resolved datasource configuration.
    #[derive(Debug, Clone)]
    pub struct DatasourceConfig {
        pub name: String,
        pub provider: DatabaseProvider,
        pub url: String,
    }
}

serde_derive! {
    /// Resolved generator configuration.
    #[derive(Debug, Clone)]
    pub struct GeneratorConfig {
        pub name: String,
        pub output: String,
    }
}

serde_derive! {
    /// A validated enum definition.
    #[derive(Debug, Clone)]
    pub struct Enum {
        pub name: String,
        pub db_name: String,
        pub variants: Vec<String>,
    }
}

serde_derive! {
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
}

serde_derive! {
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
}

impl Field {
    /// Returns true if this field is a scalar (stored in the database), not a relation.
    #[must_use]
    pub fn is_scalar(&self) -> bool {
        !matches!(self.field_type, FieldKind::Model(_)) && !self.is_list
    }

    /// Returns true if this field has a server-side default and can be omitted on create.
    #[must_use]
    pub fn has_default(&self) -> bool {
        self.default.is_some() || self.is_updated_at
    }
}

serde_derive! {
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
}

serde_derive! {
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
}

serde_derive! {
    /// The cardinality of a relation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RelationType {
        OneToOne,
        OneToMany,
        ManyToOne,
        ManyToMany,
    }
}

serde_derive! {
    /// Primary key definition.
    #[derive(Debug, Clone)]
    pub struct PrimaryKey {
        pub fields: Vec<String>,
    }
}

impl PrimaryKey {
    /// Returns true if this is a composite (multi-field) primary key.
    #[must_use]
    pub fn is_composite(&self) -> bool {
        self.fields.len() > 1
    }
}

serde_derive! {
    /// An index definition.
    #[derive(Debug, Clone)]
    pub struct Index {
        pub fields: Vec<String>,
    }
}

serde_derive! {
    /// A unique constraint.
    #[derive(Debug, Clone)]
    pub struct UniqueConstraint {
        pub fields: Vec<String>,
    }
}
