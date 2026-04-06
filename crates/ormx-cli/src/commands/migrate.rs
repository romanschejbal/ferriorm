use ormx_migrate::MigrationStrategy;
use std::path::Path;

pub async fn dev(schema_path: &str, name: Option<&str>, use_snapshot: bool) -> miette::Result<()> {
    let source = std::fs::read_to_string(schema_path)
        .map_err(|e| miette::miette!("Failed to read {schema_path}: {e}"))?;

    let schema = ormx_parser::parse_and_validate(&source)
        .map_err(|e| miette::miette!("Schema error: {e}"))?;

    let strategy = if use_snapshot {
        println!("Using snapshot strategy (offline mode)");
        MigrationStrategy::Snapshot
    } else {
        println!("Using shadow database strategy");
        MigrationStrategy::ShadowDatabase
    };

    let url = resolve_url(&schema.datasource.url)?;
    let migrations_dir = Path::new("migrations").to_path_buf();
    let runner =
        ormx_migrate::MigrationRunner::new(migrations_dir, schema.datasource.provider, strategy);

    let migration_name = name.unwrap_or("migration");

    println!("Diffing schema...");
    match runner
        .create_migration(&schema, migration_name, Some(&url))
        .await
    {
        Ok(Some(dir)) => {
            println!(
                "Created migration: {}",
                dir.file_name().unwrap().to_string_lossy()
            );

            let pool = sqlx::PgPool::connect(&url)
                .await
                .map_err(|e| miette::miette!("Failed to connect to database: {e}"))?;

            let applied = runner
                .apply_pending(&pool)
                .await
                .map_err(|e| miette::miette!("Failed to apply migration: {e}"))?;

            for name in &applied {
                println!("Applied: {name}");
            }

            pool.close().await;

            println!("Regenerating client...");
            super::generate::run(schema_path).await?;
        }
        Ok(None) => {
            println!("No schema changes detected.");
        }
        Err(e) => {
            return Err(miette::miette!("Failed to create migration: {e}"));
        }
    }

    Ok(())
}

pub async fn deploy(schema_path: &str) -> miette::Result<()> {
    let source = std::fs::read_to_string(schema_path)
        .map_err(|e| miette::miette!("Failed to read {schema_path}: {e}"))?;

    let schema = ormx_parser::parse_and_validate(&source)
        .map_err(|e| miette::miette!("Schema error: {e}"))?;

    let url = resolve_url(&schema.datasource.url)?;
    let pool = sqlx::PgPool::connect(&url)
        .await
        .map_err(|e| miette::miette!("Failed to connect to database: {e}"))?;

    let migrations_dir = Path::new("migrations").to_path_buf();
    let runner = ormx_migrate::MigrationRunner::new(
        migrations_dir,
        schema.datasource.provider,
        MigrationStrategy::Snapshot, // deploy doesn't need shadow DB
    );

    let applied = runner
        .apply_pending(&pool)
        .await
        .map_err(|e| miette::miette!("Failed to apply migrations: {e}"))?;

    if applied.is_empty() {
        println!("All migrations already applied.");
    } else {
        for name in &applied {
            println!("Applied: {name}");
        }
        println!("Applied {} migration(s).", applied.len());
    }

    pool.close().await;
    Ok(())
}

pub async fn status(schema_path: &str) -> miette::Result<()> {
    let source = std::fs::read_to_string(schema_path)
        .map_err(|e| miette::miette!("Failed to read {schema_path}: {e}"))?;

    let schema = ormx_parser::parse_and_validate(&source)
        .map_err(|e| miette::miette!("Schema error: {e}"))?;

    let url = resolve_url(&schema.datasource.url)?;
    let pool = sqlx::PgPool::connect(&url)
        .await
        .map_err(|e| miette::miette!("Failed to connect to database: {e}"))?;

    let migrations_dir = Path::new("migrations").to_path_buf();
    let runner = ormx_migrate::MigrationRunner::new(
        migrations_dir,
        schema.datasource.provider,
        MigrationStrategy::Snapshot,
    );

    let statuses = runner
        .status(&pool)
        .await
        .map_err(|e| miette::miette!("Failed to get status: {e}"))?;

    if statuses.is_empty() {
        println!("No migrations found.");
    } else {
        for s in &statuses {
            let status = if s.applied {
                format!("applied at {}", s.applied_at.unwrap())
            } else {
                "pending".to_string()
            };
            println!("  {} - {}", s.name, status);
        }
    }

    pool.close().await;
    Ok(())
}

pub fn resolve_url(url: &str) -> miette::Result<String> {
    if url.starts_with("${env:") && url.ends_with('}') {
        let var_name = &url[6..url.len() - 1];
        std::env::var(var_name)
            .map_err(|_| miette::miette!("Environment variable '{var_name}' not set"))
    } else {
        Ok(url.to_string())
    }
}
