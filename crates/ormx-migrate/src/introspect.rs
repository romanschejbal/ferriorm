//! Database introspection: reads a live database and converts its structure
//! into the [`Schema`] IR.
//!
//! This is the foundation for both shadow database diffing and the `ormx db pull`
//! command. For PostgreSQL, it queries `information_schema` and `pg_catalog`.
//! For SQLite, it uses `PRAGMA table_info`, `PRAGMA foreign_key_list`, and
//! `PRAGMA index_list`.

use ormx_core::ast::{DefaultValue, LiteralValue, ReferentialAction};
use ormx_core::schema::*;
use ormx_core::types::{DatabaseProvider, ScalarType};
use ormx_core::utils::{to_camel_case, to_pascal_case};

#[cfg(feature = "postgres")]
use sqlx::PgPool;

#[cfg(feature = "sqlite")]
use sqlx::SqlitePool;

/// Introspect a PostgreSQL database and produce a Schema IR.
#[cfg(feature = "postgres")]
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

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgEnum {
    typname: String,
    enumlabel: String,
}

#[cfg(feature = "postgres")]
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

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgColumn {
    table_name: String,
    column_name: String,
    data_type: String,
    udt_name: String,
    is_nullable: String,
    column_default: Option<String>,
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgConstraint {
    table_name: String,
    #[allow(dead_code)]
    constraint_name: String,
    constraint_type: String,
    column_name: Option<String>,
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgForeignKey {
    table_name: String,
    column_name: String,
    foreign_table_name: String,
    foreign_column_name: String,
    delete_rule: String,
    update_rule: String,
}

#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgIndex {
    tablename: String,
    #[allow(dead_code)]
    indexname: String,
    indexdef: String,
}

#[cfg(feature = "postgres")]
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

#[cfg(feature = "postgres")]
fn pg_type_to_field_kind(data_type: &str, udt_name: &str, enums: &[Enum]) -> FieldKind {
    // Check if it's a user-defined enum
    if data_type == "USER-DEFINED"
        && let Some(e) = enums.iter().find(|e| e.db_name == udt_name)
    {
        return FieldKind::Enum(e.name.clone());
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

#[cfg(feature = "postgres")]
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
    if let Some(rest) = d.strip_prefix('\'') {
        let end = rest.find('\'')?;
        let val = &rest[..end];
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

#[cfg(feature = "postgres")]
fn parse_index_columns(indexdef: &str) -> Vec<String> {
    // indexdef looks like: CREATE INDEX idx_name ON table_name USING btree (col1, col2)
    if let Some(start) = indexdef.find('(')
        && let Some(end) = indexdef.rfind(')')
    {
        return indexdef[start + 1..end]
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect();
    }
    vec![]
}

// ─── SQLite introspection ──────────────────────────────────────────

/// Introspect a SQLite database and produce a Schema IR.
///
/// SQLite has no real enum types — all "enum" columns are just TEXT.
/// Foreign keys are read via PRAGMA foreign_key_list.
/// Indexes are read via PRAGMA index_list + PRAGMA index_info.
#[cfg(feature = "sqlite")]
pub async fn introspect_sqlite(pool: &SqlitePool) -> Result<Schema, sqlx::Error> {
    let tables = introspect_sqlite_tables(pool).await?;

    Ok(Schema {
        datasource: DatasourceConfig {
            name: "db".into(),
            provider: DatabaseProvider::SQLite,
            url: String::new(),
        },
        generators: vec![],
        enums: vec![], // SQLite has no enum types
        models: tables,
    })
}

/// Row from sqlite_master for tables.
#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SqliteTable {
    name: String,
}

/// Row from PRAGMA table_info(table_name).
#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SqliteColumnInfo {
    name: String,
    #[sqlx(rename = "type")]
    col_type: String,
    notnull: i32,
    dflt_value: Option<String>,
    pk: i32,
}

/// Row from PRAGMA foreign_key_list(table_name).
#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SqliteForeignKey {
    table: String,
    from: String,
    to: String,
    on_delete: String,
    on_update: String,
}

/// Row from PRAGMA index_list(table_name).
#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SqliteIndexListEntry {
    name: String,
    unique: i32,
    origin: String,
}

/// Row from PRAGMA index_info(index_name).
#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SqliteIndexInfo {
    name: Option<String>,
}

#[cfg(feature = "sqlite")]
async fn introspect_sqlite_tables(pool: &SqlitePool) -> Result<Vec<Model>, sqlx::Error> {
    // List all user tables (exclude internal sqlite_ tables and _ormx_ tables)
    let tables = sqlx::query_as::<_, SqliteTable>(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '\\_%' ESCAPE '\\'",
    )
    .fetch_all(pool)
    .await?;

    let mut models = Vec::new();

    for table in &tables {
        let table_name = &table.name;

        // Get column info
        let columns = sqlx::query_as::<_, SqliteColumnInfo>(&format!(
            "PRAGMA table_info(\"{}\")",
            table_name
        ))
        .fetch_all(pool)
        .await?;

        // Get foreign keys
        let foreign_keys = sqlx::query_as::<_, SqliteForeignKey>(&format!(
            "PRAGMA foreign_key_list(\"{}\")",
            table_name
        ))
        .fetch_all(pool)
        .await?;

        // Get indexes
        let index_list = sqlx::query_as::<_, SqliteIndexListEntry>(&format!(
            "PRAGMA index_list(\"{}\")",
            table_name
        ))
        .fetch_all(pool)
        .await?;

        // Determine primary key columns
        let pk_columns: Vec<String> = columns
            .iter()
            .filter(|c| c.pk > 0)
            .map(|c| c.name.clone())
            .collect();

        // Determine unique columns from unique indexes (origin 'u' = user-created unique)
        let mut unique_columns: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for idx in &index_list {
            if idx.unique == 1 && idx.origin != "pk" {
                // Get columns in this index
                let idx_cols = sqlx::query_as::<_, SqliteIndexInfo>(&format!(
                    "PRAGMA index_info(\"{}\")",
                    idx.name
                ))
                .fetch_all(pool)
                .await?;
                // Only mark as unique if single-column index
                if idx_cols.len() == 1
                    && let Some(col_name) = &idx_cols[0].name
                {
                    unique_columns.insert(col_name.clone());
                }
            }
        }

        // Build fields
        let mut fields = Vec::new();
        for col in &columns {
            let is_id = col.pk > 0;
            let is_unique = unique_columns.contains(&col.name);
            let is_nullable = col.notnull == 0 && !is_id;

            let field_type = sqlite_type_to_field_kind(&col.col_type);
            let default = col
                .dflt_value
                .as_ref()
                .and_then(|d| parse_sqlite_default(d));
            let is_updated_at = col
                .dflt_value
                .as_ref()
                .is_some_and(|d| d.contains("CURRENT_TIMESTAMP"));

            // Check if this column is a foreign key
            let relation = foreign_keys
                .iter()
                .find(|fk| fk.from == col.name)
                .map(|fk| ResolvedRelation {
                    related_model: to_pascal_case(&fk.table),
                    relation_type: RelationType::ManyToOne,
                    fields: vec![col.name.clone()],
                    references: vec![fk.to.clone()],
                    on_delete: parse_referential_action(&fk.on_delete),
                    on_update: parse_referential_action(&fk.on_update),
                });

            fields.push(Field {
                name: to_camel_case(&col.name),
                db_name: col.name.clone(),
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

        // Parse indexes (non-unique, non-pk indexes)
        let mut model_indexes = Vec::new();
        for idx in &index_list {
            if idx.origin == "c" && idx.unique == 0 {
                let idx_cols = sqlx::query_as::<_, SqliteIndexInfo>(&format!(
                    "PRAGMA index_info(\"{}\")",
                    idx.name
                ))
                .fetch_all(pool)
                .await?;
                let col_names: Vec<String> =
                    idx_cols.iter().filter_map(|c| c.name.clone()).collect();
                if !col_names.is_empty() {
                    model_indexes.push(Index { fields: col_names });
                }
            }
        }

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

#[cfg(feature = "sqlite")]
fn sqlite_type_to_field_kind(col_type: &str) -> FieldKind {
    let upper = col_type.to_uppercase();
    let scalar = match upper.as_str() {
        "TEXT" | "VARCHAR" | "CHAR" | "CLOB" => ScalarType::String,
        "INTEGER" | "INT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => ScalarType::Int,
        "BIGINT" => ScalarType::BigInt,
        "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" => ScalarType::Float,
        "NUMERIC" | "DECIMAL" => ScalarType::Decimal,
        "BOOLEAN" | "BOOL" => ScalarType::Boolean,
        "BLOB" => ScalarType::Bytes,
        "DATETIME" | "TIMESTAMP" => ScalarType::DateTime,
        _ => {
            // SQLite type affinity rules
            if upper.contains("INT") {
                ScalarType::Int
            } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
                ScalarType::String
            } else if upper.contains("BLOB") {
                ScalarType::Bytes
            } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
                ScalarType::Float
            } else {
                ScalarType::String // NUMERIC affinity fallback
            }
        }
    };

    FieldKind::Scalar(scalar)
}

#[cfg(feature = "sqlite")]
fn parse_sqlite_default(default: &str) -> Option<DefaultValue> {
    let d = default.trim();

    if d.eq_ignore_ascii_case("CURRENT_TIMESTAMP") {
        return Some(DefaultValue::Now);
    }

    // String literal: 'value'
    if d.starts_with('\'') && d.ends_with('\'') && d.len() >= 2 {
        let val = &d[1..d.len() - 1];
        return Some(DefaultValue::Literal(LiteralValue::String(val.to_string())));
    }

    // Boolean (SQLite uses 0/1 but also supports TRUE/FALSE keywords)
    if d.eq_ignore_ascii_case("TRUE") || d == "1" {
        return Some(DefaultValue::Literal(LiteralValue::Bool(true)));
    }
    if d.eq_ignore_ascii_case("FALSE") || d == "0" {
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

// ─── Shared helpers ────────────────────────────────────────────────

fn parse_referential_action(rule: &str) -> ReferentialAction {
    match rule {
        "CASCADE" => ReferentialAction::Cascade,
        "SET NULL" => ReferentialAction::SetNull,
        "SET DEFAULT" => ReferentialAction::SetDefault,
        "RESTRICT" => ReferentialAction::Restrict,
        _ => ReferentialAction::NoAction,
    }
}
