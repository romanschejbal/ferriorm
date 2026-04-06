use ormx_core::types::DatabaseProvider;
use ormx_core::utils::to_snake_case;
use std::path::Path;

/// Introspect a live database and generate a schema.ormx file from it.
pub async fn pull(schema_path: &str) -> miette::Result<()> {
    let source = std::fs::read_to_string(schema_path)
        .map_err(|e| miette::miette!("Failed to read {schema_path}: {e}"))?;

    let schema = ormx_parser::parse_and_validate(&source)
        .map_err(|e| miette::miette!("Schema error: {e}"))?;

    let url = super::migrate::resolve_url(&schema.datasource.url)?;

    println!("Connecting to database...");
    let db_schema = match schema.datasource.provider {
        DatabaseProvider::PostgreSQL => {
            let pool = sqlx::PgPool::connect(&url)
                .await
                .map_err(|e| miette::miette!("Failed to connect: {e}"))?;
            println!("Introspecting PostgreSQL database...");
            let s = ormx_migrate::introspect::introspect_postgres(&pool, "public")
                .await
                .map_err(|e| miette::miette!("Introspection failed: {e}"))?;
            pool.close().await;
            s
        }
        DatabaseProvider::SQLite => {
            let pool = sqlx::SqlitePool::connect(&url)
                .await
                .map_err(|e| miette::miette!("Failed to connect: {e}"))?;
            println!("Introspecting SQLite database...");
            let s = ormx_migrate::introspect::introspect_sqlite(&pool)
                .await
                .map_err(|e| miette::miette!("Introspection failed: {e}"))?;
            pool.close().await;
            s
        }
        _ => return Err(miette::miette!("Unsupported database provider")),
    };

    // Generate .ormx schema text from introspected schema
    let schema_text = schema_to_ormx(&db_schema, &schema.datasource.url);

    let output_path = Path::new(schema_path);
    let backup_path = output_path.with_extension("ormx.bak");

    // Back up existing schema
    if output_path.exists() {
        std::fs::copy(output_path, &backup_path)
            .map_err(|e| miette::miette!("Failed to backup schema: {e}"))?;
        println!("Backed up existing schema to {}", backup_path.display());
    }

    std::fs::write(output_path, &schema_text)
        .map_err(|e| miette::miette!("Failed to write schema: {e}"))?;

    println!(
        "Pulled schema: {} models, {} enums",
        db_schema.models.len(),
        db_schema.enums.len()
    );
    println!("Written to {}", output_path.display());

    Ok(())
}

/// Convert a Schema IR back to .ormx schema text.
fn schema_to_ormx(schema: &ormx_core::schema::Schema, _url: &str) -> String {
    let mut out = String::new();

    // Datasource
    out.push_str(&format!(
        "datasource {} {{\n  provider = \"{}\"\n  url      = env(\"DATABASE_URL\")\n}}\n\n",
        schema.datasource.name,
        schema.datasource.provider.as_str(),
    ));

    // Generator
    out.push_str("generator client {\n  output = \"./src/generated\"\n}\n\n");

    // Enums
    for e in &schema.enums {
        out.push_str(&format!("enum {} {{\n", e.name));
        for v in &e.variants {
            out.push_str(&format!("  {v}\n"));
        }
        out.push_str("}\n\n");
    }

    // Models
    for model in &schema.models {
        out.push_str(&format!("model {} {{\n", model.name));
        for field in &model.fields {
            let type_name = match &field.field_type {
                ormx_core::schema::FieldKind::Scalar(s) => format!("{s}"),
                ormx_core::schema::FieldKind::Enum(name) => name.clone(),
                ormx_core::schema::FieldKind::Model(name) => name.clone(),
            };
            let optional = if field.is_optional { "?" } else { "" };
            let list = if field.is_list { "[]" } else { "" };

            let mut attrs = Vec::new();
            if field.is_id {
                attrs.push("@id".to_string());
            }
            if field.is_unique {
                attrs.push("@unique".to_string());
            }
            if let Some(default) = &field.default {
                attrs.push(format!("@default({})", default_to_string(default)));
            }
            if field.is_updated_at {
                attrs.push("@updatedAt".to_string());
            }

            let attrs_str = if attrs.is_empty() {
                String::new()
            } else {
                format!(" {}", attrs.join(" "))
            };

            out.push_str(&format!(
                "  {} {}{}{}{}\n",
                field.name, type_name, list, optional, attrs_str
            ));
        }

        if model.db_name != format!("{}s", to_snake_case(&model.name)) {
            out.push_str(&format!("\n  @@map(\"{}\")\n", model.db_name));
        }

        out.push_str("}\n\n");
    }

    out
}

fn default_to_string(d: &ormx_core::ast::DefaultValue) -> String {
    use ormx_core::ast::{DefaultValue, LiteralValue};
    match d {
        DefaultValue::Uuid => "uuid()".into(),
        DefaultValue::Cuid => "cuid()".into(),
        DefaultValue::AutoIncrement => "autoincrement()".into(),
        DefaultValue::Now => "now()".into(),
        DefaultValue::Literal(lit) => match lit {
            LiteralValue::String(s) => format!("\"{s}\""),
            LiteralValue::Int(i) => i.to_string(),
            LiteralValue::Float(f) => f.to_string(),
            LiteralValue::Bool(b) => b.to_string(),
        },
        DefaultValue::EnumVariant(v) => v.clone(),
    }
}
