//! Schema diff engine: compares two Schema IRs and produces migration steps.
//!
//! [`diff_schemas`] is the core function. It walks models, fields, enums,
//! indexes, and foreign keys in both the "from" and "to" schemas and emits a
//! list of [`MigrationStep`]s (create table, add column, alter column, etc.).
//! These steps are then rendered into SQL by the [`super::sql`] module.

use ormx_core::schema::*;
use ormx_core::utils::to_snake_case;
use std::collections::HashMap;

/// A single atomic change between two schema versions.
#[derive(Debug, Clone)]
pub enum MigrationStep {
    CreateTable(CreateTable),
    DropTable {
        name: String,
    },
    AddColumn {
        table: String,
        column: ColumnDef,
    },
    DropColumn {
        table: String,
        column: String,
    },
    AlterColumn {
        table: String,
        column: String,
        changes: ColumnChanges,
    },
    CreateIndex {
        table: String,
        name: String,
        columns: Vec<String>,
    },
    DropIndex {
        table: String,
        name: String,
    },
    AddForeignKey(ForeignKeyDef),
    DropForeignKey {
        table: String,
        name: String,
    },
    AddUniqueConstraint {
        table: String,
        name: String,
        columns: Vec<String>,
    },
    DropUniqueConstraint {
        table: String,
        name: String,
    },
    CreateEnum {
        name: String,
        variants: Vec<String>,
    },
    DropEnum {
        name: String,
    },
    AddEnumVariant {
        enum_name: String,
        variant: String,
    },
}

#[derive(Debug, Clone)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub sql_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub is_unique: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ColumnChanges {
    pub sql_type: Option<String>,
    pub nullable: Option<bool>,
    pub default: Option<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct ForeignKeyDef {
    pub table: String,
    pub constraint_name: String,
    pub column: String,
    pub referenced_table: String,
    pub referenced_column: String,
    pub on_delete: String,
    pub on_update: String,
}

/// Compare two schemas and produce the migration steps.
pub fn diff_schemas(
    from: &Schema,
    to: &Schema,
    provider: ormx_core::types::DatabaseProvider,
) -> Vec<MigrationStep> {
    let mut steps = Vec::new();

    diff_enums(&from.enums, &to.enums, &mut steps);
    diff_models(&from.models, &to.models, provider, &to.enums, &mut steps);
    diff_foreign_keys(&from.models, &to.models, &mut steps);
    diff_indexes(&from.models, &to.models, &mut steps);

    steps
}

fn diff_enums(from: &[Enum], to: &[Enum], steps: &mut Vec<MigrationStep>) {
    let from_map: HashMap<&str, &Enum> = from.iter().map(|e| (e.db_name.as_str(), e)).collect();
    let to_map: HashMap<&str, &Enum> = to.iter().map(|e| (e.db_name.as_str(), e)).collect();

    // New enums
    for (name, e) in &to_map {
        if !from_map.contains_key(name) {
            steps.push(MigrationStep::CreateEnum {
                name: e.db_name.clone(),
                variants: e.variants.clone(),
            });
        }
    }

    // Dropped enums
    for (name, e) in &from_map {
        if !to_map.contains_key(name) {
            steps.push(MigrationStep::DropEnum {
                name: e.db_name.clone(),
            });
        }
    }

    // Modified enums (new variants only — removing variants is dangerous)
    for (name, to_enum) in &to_map {
        if let Some(from_enum) = from_map.get(name) {
            for variant in &to_enum.variants {
                if !from_enum.variants.contains(variant) {
                    steps.push(MigrationStep::AddEnumVariant {
                        enum_name: to_enum.db_name.clone(),
                        variant: variant.clone(),
                    });
                }
            }
        }
    }
}

fn diff_models(
    from: &[Model],
    to: &[Model],
    provider: ormx_core::types::DatabaseProvider,
    enums: &[Enum],
    steps: &mut Vec<MigrationStep>,
) {
    let from_map: HashMap<&str, &Model> = from.iter().map(|m| (m.db_name.as_str(), m)).collect();
    let to_map: HashMap<&str, &Model> = to.iter().map(|m| (m.db_name.as_str(), m)).collect();

    // New tables
    for (name, model) in &to_map {
        if !from_map.contains_key(name) {
            steps.push(MigrationStep::CreateTable(model_to_create_table(
                model, provider, enums,
            )));
        }
    }

    // Dropped tables
    for name in from_map.keys() {
        if !to_map.contains_key(name) {
            steps.push(MigrationStep::DropTable {
                name: name.to_string(),
            });
        }
    }

    // Modified tables (column changes)
    for (name, to_model) in &to_map {
        if let Some(from_model) = from_map.get(name) {
            diff_columns(name, from_model, to_model, provider, enums, steps);
        }
    }
}

fn diff_columns(
    table: &str,
    from: &Model,
    to: &Model,
    provider: ormx_core::types::DatabaseProvider,
    enums: &[Enum],
    steps: &mut Vec<MigrationStep>,
) {
    let from_cols: HashMap<&str, &Field> = from
        .fields
        .iter()
        .filter(|f| f.is_scalar())
        .map(|f| (f.db_name.as_str(), f))
        .collect();
    let to_cols: HashMap<&str, &Field> = to
        .fields
        .iter()
        .filter(|f| f.is_scalar())
        .map(|f| (f.db_name.as_str(), f))
        .collect();

    // New columns
    for (col_name, field) in &to_cols {
        if !from_cols.contains_key(col_name) {
            steps.push(MigrationStep::AddColumn {
                table: table.to_string(),
                column: field_to_column_def(field, provider, enums),
            });
        }
    }

    // Dropped columns
    for col_name in from_cols.keys() {
        if !to_cols.contains_key(col_name) {
            steps.push(MigrationStep::DropColumn {
                table: table.to_string(),
                column: col_name.to_string(),
            });
        }
    }

    // Altered columns
    for (col_name, to_field) in &to_cols {
        if let Some(from_field) = from_cols.get(col_name) {
            let changes = diff_column(from_field, to_field, provider, enums);
            if changes.sql_type.is_some() || changes.nullable.is_some() || changes.default.is_some()
            {
                steps.push(MigrationStep::AlterColumn {
                    table: table.to_string(),
                    column: col_name.to_string(),
                    changes,
                });
            }
        }
    }
}

fn diff_column(
    from: &Field,
    to: &Field,
    provider: ormx_core::types::DatabaseProvider,
    enums: &[Enum],
) -> ColumnChanges {
    let from_type = field_sql_type(&from.field_type, provider, enums);
    let to_type = field_sql_type(&to.field_type, provider, enums);

    ColumnChanges {
        sql_type: if from_type != to_type {
            Some(to_type)
        } else {
            None
        },
        nullable: if from.is_optional != to.is_optional {
            Some(to.is_optional)
        } else {
            None
        },
        default: None, // TODO: diff defaults
    }
}

fn diff_foreign_keys(from: &[Model], to: &[Model], steps: &mut Vec<MigrationStep>) {
    let from_fks = collect_foreign_keys(from);
    let to_fks = collect_foreign_keys(to);

    for fk in &to_fks {
        if !from_fks
            .iter()
            .any(|f| f.constraint_name == fk.constraint_name)
        {
            steps.push(MigrationStep::AddForeignKey(fk.clone()));
        }
    }

    for fk in &from_fks {
        if !to_fks
            .iter()
            .any(|f| f.constraint_name == fk.constraint_name)
        {
            steps.push(MigrationStep::DropForeignKey {
                table: fk.table.clone(),
                name: fk.constraint_name.clone(),
            });
        }
    }
}

fn collect_foreign_keys(models: &[Model]) -> Vec<ForeignKeyDef> {
    let mut fks = Vec::new();
    let model_map: HashMap<&str, &Model> = models.iter().map(|m| (m.name.as_str(), m)).collect();

    for model in models {
        for field in &model.fields {
            if let Some(rel) = &field.relation
                && !rel.fields.is_empty()
                && let Some(related) = model_map.get(rel.related_model.as_str())
            {
                let fk_col = to_snake_case(&rel.fields[0]);
                let ref_col = to_snake_case(&rel.references[0]);
                fks.push(ForeignKeyDef {
                    table: model.db_name.clone(),
                    constraint_name: format!("fk_{}_{}_{}", model.db_name, related.db_name, fk_col),
                    column: fk_col,
                    referenced_table: related.db_name.clone(),
                    referenced_column: ref_col,
                    on_delete: referential_action_sql(rel.on_delete),
                    on_update: referential_action_sql(rel.on_update),
                });
            }
        }
    }
    fks
}

fn diff_indexes(from: &[Model], to: &[Model], steps: &mut Vec<MigrationStep>) {
    let from_map: HashMap<&str, &Model> = from.iter().map(|m| (m.db_name.as_str(), m)).collect();
    let to_map: HashMap<&str, &Model> = to.iter().map(|m| (m.db_name.as_str(), m)).collect();

    for (name, to_model) in &to_map {
        let from_indexes: Vec<&Index> = from_map
            .get(name)
            .map(|m| m.indexes.iter().collect())
            .unwrap_or_default();

        for idx in &to_model.indexes {
            let idx_name = format!(
                "idx_{}_{}",
                name,
                idx.fields
                    .iter()
                    .map(|f| to_snake_case(f))
                    .collect::<Vec<_>>()
                    .join("_")
            );
            if !from_indexes.iter().any(|fi| fi.fields == idx.fields) {
                steps.push(MigrationStep::CreateIndex {
                    table: name.to_string(),
                    name: idx_name,
                    columns: idx.fields.iter().map(|f| to_snake_case(f)).collect(),
                });
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────

fn model_to_create_table(
    model: &Model,
    provider: ormx_core::types::DatabaseProvider,
    enums: &[Enum],
) -> CreateTable {
    CreateTable {
        name: model.db_name.clone(),
        columns: model
            .fields
            .iter()
            .filter(|f| f.is_scalar())
            .map(|f| field_to_column_def(f, provider, enums))
            .collect(),
        primary_key: model
            .primary_key
            .fields
            .iter()
            .map(|f| to_snake_case(f))
            .collect(),
    }
}

fn field_to_column_def(
    field: &Field,
    provider: ormx_core::types::DatabaseProvider,
    enums: &[Enum],
) -> ColumnDef {
    ColumnDef {
        name: field.db_name.clone(),
        sql_type: field_sql_type(&field.field_type, provider, enums),
        nullable: field.is_optional,
        default: field_default_sql(field, provider),
        is_unique: field.is_unique,
    }
}

fn field_sql_type(
    field_type: &FieldKind,
    provider: ormx_core::types::DatabaseProvider,
    enums: &[Enum],
) -> String {
    match field_type {
        FieldKind::Scalar(scalar) => match provider {
            ormx_core::types::DatabaseProvider::PostgreSQL => scalar.postgres_type().to_string(),
            ormx_core::types::DatabaseProvider::SQLite => scalar.sqlite_type().to_string(),
            ormx_core::types::DatabaseProvider::MySQL => scalar.postgres_type().to_string(), // TODO
        },
        FieldKind::Enum(name) => {
            let db_name = enums
                .iter()
                .find(|e| e.name == *name)
                .map(|e| e.db_name.clone())
                .unwrap_or_else(|| to_snake_case(name));
            match provider {
                ormx_core::types::DatabaseProvider::PostgreSQL => db_name,
                _ => "TEXT".to_string(),
            }
        }
        FieldKind::Model(_) => "TEXT".to_string(), // shouldn't happen for scalar fields
    }
}

fn field_default_sql(
    field: &Field,
    provider: ormx_core::types::DatabaseProvider,
) -> Option<String> {
    use ormx_core::ast::{DefaultValue, LiteralValue};

    field.default.as_ref().map(|d| match d {
        DefaultValue::Uuid => match provider {
            ormx_core::types::DatabaseProvider::PostgreSQL => "gen_random_uuid()".to_string(),
            _ => "''".to_string(), // SQLite doesn't have built-in UUID
        },
        DefaultValue::AutoIncrement => "".to_string(), // handled by SERIAL type
        DefaultValue::Now => match provider {
            ormx_core::types::DatabaseProvider::PostgreSQL => "NOW()".to_string(),
            _ => "CURRENT_TIMESTAMP".to_string(),
        },
        DefaultValue::Cuid => "''".to_string(),
        DefaultValue::Literal(lit) => match lit {
            LiteralValue::String(s) => format!("'{s}'"),
            LiteralValue::Int(i) => i.to_string(),
            LiteralValue::Float(f) => f.to_string(),
            LiteralValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        },
        DefaultValue::EnumVariant(v) => format!("'{}'", to_snake_case(v)),
    })
}

fn referential_action_sql(action: ormx_core::ast::ReferentialAction) -> String {
    match action {
        ormx_core::ast::ReferentialAction::Cascade => "CASCADE".into(),
        ormx_core::ast::ReferentialAction::Restrict => "RESTRICT".into(),
        ormx_core::ast::ReferentialAction::NoAction => "NO ACTION".into(),
        ormx_core::ast::ReferentialAction::SetNull => "SET NULL".into(),
        ormx_core::ast::ReferentialAction::SetDefault => "SET DEFAULT".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ormx_core::types::{DatabaseProvider, ScalarType};
    use ormx_core::utils::to_snake_case;

    fn make_schema(models: Vec<Model>, enums: Vec<Enum>) -> Schema {
        Schema {
            datasource: ormx_core::schema::DatasourceConfig {
                name: "db".into(),
                provider: DatabaseProvider::PostgreSQL,
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

    #[test]
    fn test_diff_create_table() {
        let from = make_schema(vec![], vec![]);
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

        let steps = diff_schemas(&from, &to, DatabaseProvider::PostgreSQL);
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], MigrationStep::CreateTable(ct) if ct.name == "users"));

        if let MigrationStep::CreateTable(ct) = &steps[0] {
            assert_eq!(ct.columns.len(), 2);
            assert_eq!(ct.primary_key, vec!["id"]);
        }
    }

    #[test]
    fn test_diff_drop_table() {
        let from = make_schema(
            vec![make_model(
                "User",
                "users",
                vec![make_field("id", ScalarType::String, true)],
            )],
            vec![],
        );
        let to = make_schema(vec![], vec![]);

        let steps = diff_schemas(&from, &to, DatabaseProvider::PostgreSQL);
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], MigrationStep::DropTable { name } if name == "users"));
    }

    #[test]
    fn test_diff_add_column() {
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
                    make_field("email", ScalarType::String, false),
                ],
            )],
            vec![],
        );

        let steps = diff_schemas(&from, &to, DatabaseProvider::PostgreSQL);
        assert_eq!(steps.len(), 1);
        assert!(
            matches!(&steps[0], MigrationStep::AddColumn { table, column } if table == "users" && column.name == "email")
        );
    }

    #[test]
    fn test_diff_no_changes() {
        let schema = make_schema(
            vec![make_model(
                "User",
                "users",
                vec![make_field("id", ScalarType::String, true)],
            )],
            vec![],
        );

        let steps = diff_schemas(&schema, &schema, DatabaseProvider::PostgreSQL);
        assert!(steps.is_empty());
    }

    #[test]
    fn test_diff_create_enum() {
        let from = make_schema(vec![], vec![]);
        let to = make_schema(
            vec![],
            vec![Enum {
                name: "Role".into(),
                db_name: "role".into(),
                variants: vec!["User".into(), "Admin".into()],
            }],
        );

        let steps = diff_schemas(&from, &to, DatabaseProvider::PostgreSQL);
        assert_eq!(steps.len(), 1);
        assert!(
            matches!(&steps[0], MigrationStep::CreateEnum { name, variants }
            if name == "role" && variants.len() == 2)
        );
    }
}
