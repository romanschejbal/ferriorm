//! Database introspection: reads the actual database schema and converts
//! it to a Schema IR. This is the foundation for shadow database diffing.

use ormx_core::ast::{DefaultValue, LiteralValue, ReferentialAction};
use ormx_core::schema::*;
use ormx_core::types::{DatabaseProvider, ScalarType};
use sqlx::PgPool;

/// Introspect a PostgreSQL database and produce a Schema IR.
pub async fn introspect_postgres(pool: &PgPool, schema_name: &str) -> Result<Schema, sqlx::Error> {
    let enums = introspect_enums(pool, schema_name).await?;
    let models = introspect_tables(pool, schema_name, &enums).await?;

    Ok(Schema {
        datasource: DatasourceConfig {
            name: "db".into(),
            provider: DatabaseProvider::PostgreSQL,
            url: String::new(),
        },
        generators: vec![],
        enums,
        models,
    })
}

#[derive(sqlx::FromRow)]
struct PgEnum {
    typname: String,
    enumlabel: String,
}

async fn introspect_enums(pool: &PgPool, schema_name: &str) -> Result<Vec<Enum>, sqlx::Error> {
    let rows = sqlx::query_as::<_, PgEnum>(
        r#"
        SELECT t.typname, e.enumlabel
        FROM pg_type t
        JOIN pg_enum e ON t.oid = e.enumtypid
        JOIN pg_namespace n ON t.typnamespace = n.oid
        WHERE n.nspname = $1
        ORDER BY t.typname, e.enumsortorder
        "#,
    )
    .bind(schema_name)
    .fetch_all(pool)
    .await?;

    let mut enum_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for row in rows {
        enum_map
            .entry(row.typname.clone())
            .or_default()
            .push(row.enumlabel);
    }

    Ok(enum_map
        .into_iter()
        .map(|(name, variants)| {
            let pascal_name = to_pascal_case(&name);
            Enum {
                name: pascal_name,
                db_name: name,
                variants: variants.into_iter().map(|v| to_pascal_case(&v)).collect(),
            }
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct PgColumn {
    table_name: String,
    column_name: String,
    data_type: String,
    udt_name: String,
    is_nullable: String,
    column_default: Option<String>,
}

#[derive(sqlx::FromRow)]
struct PgConstraint {
    table_name: String,
    constraint_name: String,
    constraint_type: String,
    column_name: Option<String>,
}

#[derive(sqlx::FromRow)]
struct PgForeignKey {
    table_name: String,
    column_name: String,
    foreign_table_name: String,
    foreign_column_name: String,
    delete_rule: String,
    update_rule: String,
}

#[derive(sqlx::FromRow)]
struct PgIndex {
    tablename: String,
    indexname: String,
    indexdef: String,
}

async fn introspect_tables(
    pool: &PgPool,
    schema_name: &str,
    enums: &[Enum],
) -> Result<Vec<Model>, sqlx::Error> {
    // Get all columns
    let columns = sqlx::query_as::<_, PgColumn>(
        r#"
        SELECT table_name, column_name, data_type, udt_name, is_nullable, column_default
        FROM information_schema.columns
        WHERE table_schema = $1
          AND table_name NOT LIKE '\_%'
        ORDER BY table_name, ordinal_position
        "#,
    )
    .bind(schema_name)
    .fetch_all(pool)
    .await?;

    // Get primary keys and unique constraints
    let constraints = sqlx::query_as::<_, PgConstraint>(
        r#"
        SELECT tc.table_name, tc.constraint_name, tc.constraint_type, kcu.column_name
        FROM information_schema.table_constraints tc
        JOIN information_schema.key_column_usage kcu
            ON tc.constraint_name = kcu.constraint_name
            AND tc.table_schema = kcu.table_schema
        WHERE tc.table_schema = $1
          AND tc.constraint_type IN ('PRIMARY KEY', 'UNIQUE')
        ORDER BY tc.table_name, kcu.ordinal_position
        "#,
    )
    .bind(schema_name)
    .fetch_all(pool)
    .await?;

    // Get foreign keys
    let foreign_keys = sqlx::query_as::<_, PgForeignKey>(
        r#"
        SELECT
            kcu.table_name,
            kcu.column_name,
            ccu.table_name AS foreign_table_name,
            ccu.column_name AS foreign_column_name,
            rc.delete_rule,
            rc.update_rule
        FROM information_schema.key_column_usage kcu
        JOIN information_schema.referential_constraints rc
            ON kcu.constraint_name = rc.constraint_name
            AND kcu.table_schema = rc.constraint_schema
        JOIN information_schema.constraint_column_usage ccu
            ON rc.unique_constraint_name = ccu.constraint_name
            AND rc.unique_constraint_schema = ccu.constraint_schema
        WHERE kcu.table_schema = $1
        "#,
    )
    .bind(schema_name)
    .fetch_all(pool)
    .await?;

    // Get indexes
    let indexes = sqlx::query_as::<_, PgIndex>(
        r#"
        SELECT tablename, indexname, indexdef
        FROM pg_indexes
        WHERE schemaname = $1
          AND indexname NOT LIKE '%_pkey'
          AND indexname NOT LIKE '%_key'
        "#,
    )
    .bind(schema_name)
    .fetch_all(pool)
    .await?;

    // Group by table
    let mut table_columns: std::collections::HashMap<String, Vec<&PgColumn>> =
        std::collections::HashMap::new();
    for col in &columns {
        table_columns
            .entry(col.table_name.clone())
            .or_default()
            .push(col);
    }

    let mut models = Vec::new();
    for (table_name, cols) in &table_columns {
        // Find primary key columns
        let pk_columns: Vec<String> = constraints
            .iter()
            .filter(|c| c.table_name == *table_name && c.constraint_type == "PRIMARY KEY")
            .filter_map(|c| c.column_name.clone())
            .collect();

        // Find unique columns
        let unique_columns: std::collections::HashSet<String> = constraints
            .iter()
            .filter(|c| c.table_name == *table_name && c.constraint_type == "UNIQUE")
            .filter_map(|c| c.column_name.clone())
            .collect();

        // Build fields
        let mut fields = Vec::new();
        for col in cols {
            let is_id = pk_columns.contains(&col.column_name);
            let is_unique = unique_columns.contains(&col.column_name);
            let is_nullable = col.is_nullable == "YES";

            let field_type = pg_type_to_field_kind(&col.data_type, &col.udt_name, enums);
            let default = col
                .column_default
                .as_ref()
                .and_then(|d| parse_pg_default(d));
            let is_updated_at = col
                .column_default
                .as_ref()
                .is_some_and(|d| d.contains("now()") || d.contains("CURRENT_TIMESTAMP"));

            // Check if this column is a foreign key
            let relation = foreign_keys
                .iter()
                .find(|fk| fk.table_name == *table_name && fk.column_name == col.column_name)
                .map(|fk| ResolvedRelation {
                    related_model: to_pascal_case(&fk.foreign_table_name),
                    relation_type: RelationType::ManyToOne,
                    fields: vec![col.column_name.clone()],
                    references: vec![fk.foreign_column_name.clone()],
                    on_delete: parse_referential_action(&fk.delete_rule),
                    on_update: parse_referential_action(&fk.update_rule),
                });

            fields.push(Field {
                name: to_camel_case(&col.column_name),
                db_name: col.column_name.clone(),
                field_type,
                is_optional: is_nullable,
                is_list: false,
                is_id,
                is_unique,
                is_updated_at,
                default,
                relation,
            });
        }

        // Parse indexes
        let model_indexes: Vec<Index> = indexes
            .iter()
            .filter(|idx| idx.tablename == *table_name)
            .filter_map(|idx| {
                // Extract column names from indexdef (simplified)
                let cols = parse_index_columns(&idx.indexdef);
                if cols.is_empty() {
                    None
                } else {
                    Some(Index { fields: cols })
                }
            })
            .collect();

        models.push(Model {
            name: to_pascal_case(table_name),
            db_name: table_name.clone(),
            fields,
            primary_key: PrimaryKey { fields: pk_columns },
            indexes: model_indexes,
            unique_constraints: vec![],
        });
    }

    Ok(models)
}

fn pg_type_to_field_kind(data_type: &str, udt_name: &str, enums: &[Enum]) -> FieldKind {
    // Check if it's a user-defined enum
    if data_type == "USER-DEFINED" {
        if let Some(e) = enums.iter().find(|e| e.db_name == udt_name) {
            return FieldKind::Enum(e.name.clone());
        }
    }

    let scalar = match data_type {
        "text" | "character varying" | "varchar" | "char" | "character" | "uuid" => {
            ScalarType::String
        }
        "integer" | "int4" | "smallint" | "int2" => ScalarType::Int,
        "bigint" | "int8" => ScalarType::BigInt,
        "double precision" | "float8" | "real" | "float4" => ScalarType::Float,
        "numeric" | "decimal" => ScalarType::Decimal,
        "boolean" | "bool" => ScalarType::Boolean,
        "timestamp with time zone"
        | "timestamptz"
        | "timestamp without time zone"
        | "timestamp" => ScalarType::DateTime,
        "json" | "jsonb" => ScalarType::Json,
        "bytea" => ScalarType::Bytes,
        _ => ScalarType::String, // fallback
    };

    FieldKind::Scalar(scalar)
}

fn parse_pg_default(default: &str) -> Option<DefaultValue> {
    let d = default.trim();

    if d.contains("gen_random_uuid()") || d.contains("uuid_generate_v4()") {
        return Some(DefaultValue::Uuid);
    }
    if d.contains("now()") || d.contains("CURRENT_TIMESTAMP") {
        return Some(DefaultValue::Now);
    }
    if d.starts_with("nextval(") {
        return Some(DefaultValue::AutoIncrement);
    }

    // String literal: 'value'::type
    if d.starts_with('\'') {
        let end = d[1..].find('\'')?;
        let val = &d[1..1 + end];
        return Some(DefaultValue::Literal(LiteralValue::String(val.to_string())));
    }

    // Boolean
    if d == "true" {
        return Some(DefaultValue::Literal(LiteralValue::Bool(true)));
    }
    if d == "false" {
        return Some(DefaultValue::Literal(LiteralValue::Bool(false)));
    }

    // Numeric
    if let Ok(i) = d.parse::<i64>() {
        return Some(DefaultValue::Literal(LiteralValue::Int(i)));
    }
    if let Ok(f) = d.parse::<f64>() {
        return Some(DefaultValue::Literal(LiteralValue::Float(f)));
    }

    None
}

fn parse_referential_action(rule: &str) -> ReferentialAction {
    match rule {
        "CASCADE" => ReferentialAction::Cascade,
        "SET NULL" => ReferentialAction::SetNull,
        "SET DEFAULT" => ReferentialAction::SetDefault,
        "RESTRICT" => ReferentialAction::Restrict,
        _ => ReferentialAction::NoAction,
    }
}

fn parse_index_columns(indexdef: &str) -> Vec<String> {
    // indexdef looks like: CREATE INDEX idx_name ON table_name USING btree (col1, col2)
    if let Some(start) = indexdef.find('(') {
        if let Some(end) = indexdef.rfind(')') {
            return indexdef[start + 1..end]
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .collect();
        }
    }
    vec![]
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
