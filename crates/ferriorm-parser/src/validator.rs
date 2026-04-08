//! Semantic validation of the raw AST into the resolved Schema IR.
//!
//! The validator walks the [`ferriorm_core::ast::SchemaFile`] produced by the
//! parser and performs the following:
//!
//! - Resolves field type names to scalars, enums, or model references.
//! - Infers database table and column names from `@@map`/`@map` or `snake_case`
//!   conventions.
//! - Checks that every model has a primary key (`@id` or `@@id`).
//! - Detects duplicate model/enum names and unknown type references.
//! - Resolves relation cardinality and referential actions.
//!
//! The output is an [`ferriorm_core::schema::Schema`], the canonical IR consumed
//! by codegen and the migration engine.

use std::collections::HashSet;

use ferriorm_core::ast;
use ferriorm_core::error::CoreError;
use ferriorm_core::schema::{
    DatasourceConfig, Enum, Field, FieldKind, GeneratorConfig, Index, Model, PrimaryKey,
    RelationType, ResolvedRelation, Schema, UniqueConstraint,
};
use ferriorm_core::types::{DatabaseProvider, ScalarType};
use ferriorm_core::utils::to_snake_case;

/// Validate a parsed AST and produce a resolved Schema IR.
///
/// # Errors
///
/// Returns a [`CoreError`] if the AST has validation problems such as
/// missing primary keys, unknown types, or duplicate names.
pub fn validate(ast: &ast::SchemaFile) -> Result<Schema, CoreError> {
    let datasource = validate_datasource(ast)?;
    let generators = validate_generators(ast)?;
    let enums = validate_enums(ast)?;
    let models = validate_models(ast, &enums)?;

    Ok(Schema {
        datasource,
        generators,
        enums,
        models,
    })
}

fn validate_datasource(ast: &ast::SchemaFile) -> Result<DatasourceConfig, CoreError> {
    let ds = ast.datasource.as_ref().ok_or(CoreError::Validation {
        message: "Missing datasource block".into(),
    })?;

    let provider =
        ds.provider
            .parse::<DatabaseProvider>()
            .map_err(|_| CoreError::UnknownProvider {
                provider: ds.provider.clone(),
            })?;

    let url = match &ds.url {
        ast::StringOrEnv::Literal(s) => s.clone(),
        ast::StringOrEnv::Env(var) => format!("${{env:{var}}}"),
    };

    Ok(DatasourceConfig {
        name: ds.name.clone(),
        provider,
        url,
    })
}

fn validate_generators(ast: &ast::SchemaFile) -> Result<Vec<GeneratorConfig>, CoreError> {
    ast.generators
        .iter()
        .map(|g| {
            Ok(GeneratorConfig {
                name: g.name.clone(),
                output: g.output.clone().unwrap_or_else(|| "./src/generated".into()),
            })
        })
        .collect()
}

fn validate_enums(ast: &ast::SchemaFile) -> Result<Vec<Enum>, CoreError> {
    let mut names = HashSet::new();
    let mut result = Vec::new();

    for e in &ast.enums {
        if !names.insert(&e.name) {
            return Err(CoreError::DuplicateName {
                name: e.name.clone(),
                kind: "enum",
            });
        }

        result.push(Enum {
            name: e.name.clone(),
            db_name: e.db_name.clone().unwrap_or_else(|| to_snake_case(&e.name)),
            variants: e.variants.clone(),
        });
    }

    Ok(result)
}

fn validate_models(ast: &ast::SchemaFile, enums: &[Enum]) -> Result<Vec<Model>, CoreError> {
    let enum_names: HashSet<&str> = enums.iter().map(|e| e.name.as_str()).collect();
    let model_names: HashSet<&str> = ast.models.iter().map(|m| m.name.as_str()).collect();
    let mut seen_names = HashSet::new();

    let mut result = Vec::new();

    for model_def in &ast.models {
        if !seen_names.insert(&model_def.name) {
            return Err(CoreError::DuplicateName {
                name: model_def.name.clone(),
                kind: "model",
            });
        }

        // Check for name collision with enums
        if enum_names.contains(model_def.name.as_str()) {
            return Err(CoreError::DuplicateName {
                name: model_def.name.clone(),
                kind: "model/enum",
            });
        }

        let model = validate_model(model_def, &enum_names, &model_names)?;
        result.push(model);
    }

    Ok(result)
}

fn validate_model(
    model_def: &ast::ModelDef,
    enum_names: &HashSet<&str>,
    model_names: &HashSet<&str>,
) -> Result<Model, CoreError> {
    // Resolve @@map
    let db_name = model_def
        .attributes
        .iter()
        .find_map(|a| match a {
            ast::BlockAttribute::Map(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| to_snake_case(&model_def.name) + "s");

    let mut fields = Vec::new();
    let mut has_id_field = false;

    for field_def in &model_def.fields {
        let field = validate_field(field_def, &model_def.name, enum_names, model_names)?;
        if field.is_id {
            has_id_field = true;
        }
        fields.push(field);
    }

    // Check @@id for composite primary key
    let composite_id: Option<Vec<String>> = model_def.attributes.iter().find_map(|a| match a {
        ast::BlockAttribute::Id(fields) => Some(fields.clone()),
        _ => None,
    });

    if !has_id_field && composite_id.is_none() {
        return Err(CoreError::MissingPrimaryKey {
            model_name: model_def.name.clone(),
        });
    }

    let primary_key = if let Some(composite_fields) = composite_id {
        PrimaryKey {
            fields: composite_fields,
        }
    } else {
        let id_fields: Vec<String> = fields
            .iter()
            .filter(|f| f.is_id)
            .map(|f| f.name.clone())
            .collect();
        PrimaryKey { fields: id_fields }
    };

    // Indexes
    let indexes = model_def
        .attributes
        .iter()
        .filter_map(|a| match a {
            ast::BlockAttribute::Index(fields) => Some(Index {
                fields: fields.clone(),
            }),
            _ => None,
        })
        .collect();

    // Unique constraints (from @@unique)
    let unique_constraints = model_def
        .attributes
        .iter()
        .filter_map(|a| match a {
            ast::BlockAttribute::Unique(fields) => Some(UniqueConstraint {
                fields: fields.clone(),
            }),
            _ => None,
        })
        .collect();

    Ok(Model {
        name: model_def.name.clone(),
        db_name,
        fields,
        primary_key,
        indexes,
        unique_constraints,
    })
}

fn validate_field(
    field_def: &ast::FieldDef,
    model_name: &str,
    enum_names: &HashSet<&str>,
    model_names: &HashSet<&str>,
) -> Result<Field, CoreError> {
    let type_name = &field_def.field_type.name;

    let field_type = if let Ok(scalar) = type_name.parse::<ScalarType>() {
        FieldKind::Scalar(scalar)
    } else if enum_names.contains(type_name.as_str()) {
        FieldKind::Enum(type_name.clone())
    } else if model_names.contains(type_name.as_str()) {
        FieldKind::Model(type_name.clone())
    } else {
        return Err(CoreError::UnknownType {
            model_name: model_name.to_string(),
            field_name: field_def.name.clone(),
            type_name: type_name.clone(),
        });
    };

    let is_id = field_def
        .attributes
        .iter()
        .any(|a| matches!(a, ast::FieldAttribute::Id));
    let is_unique = field_def
        .attributes
        .iter()
        .any(|a| matches!(a, ast::FieldAttribute::Unique));
    let is_updated_at = field_def
        .attributes
        .iter()
        .any(|a| matches!(a, ast::FieldAttribute::UpdatedAt));
    let default = field_def.attributes.iter().find_map(|a| match a {
        ast::FieldAttribute::Default(d) => Some(d.clone()),
        _ => None,
    });

    // Resolve @map
    let db_name = field_def
        .attributes
        .iter()
        .find_map(|a| match a {
            ast::FieldAttribute::Map(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| to_snake_case(&field_def.name));

    // Resolve @relation
    let relation = field_def.attributes.iter().find_map(|a| match a {
        ast::FieldAttribute::Relation(rel) => {
            let relation_type = if field_def.field_type.is_list {
                RelationType::OneToMany
            } else if field_def.field_type.is_optional {
                RelationType::OneToOne
            } else {
                RelationType::ManyToOne
            };

            Some(ResolvedRelation {
                related_model: type_name.clone(),
                relation_type,
                fields: rel.fields.clone(),
                references: rel.references.clone(),
                on_delete: rel.on_delete.unwrap_or(ast::ReferentialAction::Restrict),
                on_update: rel.on_update.unwrap_or(ast::ReferentialAction::Cascade),
            })
        }
        _ => None,
    });

    Ok(Field {
        name: field_def.name.clone(),
        db_name,
        field_type,
        is_optional: field_def.field_type.is_optional,
        is_list: field_def.field_type.is_list,
        is_id,
        is_unique,
        is_updated_at,
        default,
        relation,
    })
}

#[cfg(test)]
#[allow(clippy::pedantic)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use ferriorm_core::utils::to_snake_case;

    #[test]
    fn test_validate_basic_schema() {
        let source = r#"
datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

generator client {
  output = "./src/generated"
}

enum Role {
  User
  Admin
}

model User {
  id    String @id @default(uuid())
  email String @unique
  name  String?
  role  Role   @default(User)

  @@map("users")
}
"#;

        let ast = parse(source).expect("parse");
        let schema = validate(&ast).expect("validate");

        assert_eq!(schema.datasource.provider, DatabaseProvider::PostgreSQL);
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].name, "Role");
        assert_eq!(schema.enums[0].db_name, "role");

        let user = &schema.models[0];
        assert_eq!(user.name, "User");
        assert_eq!(user.db_name, "users");
        assert_eq!(user.primary_key.fields, vec!["id"]);

        let id_field = &user.fields[0];
        assert!(id_field.is_id);
        assert_eq!(id_field.field_type, FieldKind::Scalar(ScalarType::String));

        let name_field = &user.fields[2];
        assert!(name_field.is_optional);
        assert_eq!(name_field.db_name, "name");

        let role_field = &user.fields[3];
        assert_eq!(role_field.field_type, FieldKind::Enum("Role".into()));
    }

    #[test]
    fn test_validate_missing_primary_key() {
        let source = r#"
datasource db {
  provider = "postgresql"
  url      = "postgres://localhost/test"
}

model User {
  email String
  name  String
}
"#;

        let ast = parse(source).expect("parse");
        let err = validate(&ast).unwrap_err();
        assert!(matches!(err, CoreError::MissingPrimaryKey { .. }));
    }

    #[test]
    fn test_validate_unknown_type() {
        let source = r#"
datasource db {
  provider = "postgresql"
  url      = "postgres://localhost/test"
}

model User {
  id   String @id
  role Nonexistent
}
"#;

        let ast = parse(source).expect("parse");
        let err = validate(&ast).unwrap_err();
        assert!(matches!(err, CoreError::UnknownType { .. }));
    }

    #[test]
    fn test_validate_composite_primary_key() {
        let source = r#"
datasource db {
  provider = "sqlite"
  url      = "file:./dev.db"
}

model PostTag {
  postId String
  tagId  String

  @@id([postId, tagId])
}
"#;

        let ast = parse(source).expect("parse");
        let schema = validate(&ast).expect("validate");
        let model = &schema.models[0];
        assert_eq!(model.primary_key.fields, vec!["postId", "tagId"]);
        assert!(model.primary_key.is_composite());
    }

    #[test]
    fn test_snake_case() {
        assert_eq!(to_snake_case("User"), "user");
        assert_eq!(to_snake_case("PostTag"), "post_tag");
        assert_eq!(to_snake_case("createdAt"), "created_at");
        assert_eq!(to_snake_case("HTMLParser"), "h_t_m_l_parser");
    }

    #[test]
    fn test_validate_auto_table_name() {
        let source = r#"
datasource db {
  provider = "postgresql"
  url      = "postgres://localhost/test"
}

model BlogPost {
  id String @id
}
"#;

        let ast = parse(source).expect("parse");
        let schema = validate(&ast).expect("validate");
        // Auto-generated: snake_case + "s"
        assert_eq!(schema.models[0].db_name, "blog_posts");
    }
}
