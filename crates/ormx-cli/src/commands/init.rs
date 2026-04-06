use std::fs;
use std::path::Path;

pub async fn run(provider: &str) -> miette::Result<()> {
    let schema_path = Path::new("schema.ormx");
    let migrations_dir = Path::new("migrations");

    if schema_path.exists() {
        eprintln!("schema.ormx already exists, skipping.");
    } else {
        let template = generate_template(provider);
        fs::write(schema_path, template)
            .map_err(|e| miette::miette!("Failed to write schema.ormx: {e}"))?;
        println!("Created schema.ormx");
    }

    if !migrations_dir.exists() {
        fs::create_dir_all(migrations_dir)
            .map_err(|e| miette::miette!("Failed to create migrations/: {e}"))?;
        println!("Created migrations/");
    }

    println!("\nNext steps:");
    println!("  1. Edit schema.ormx to define your models");
    println!("  2. Run `ormx generate` to generate the Rust client");

    Ok(())
}

fn generate_template(provider: &str) -> String {
    let url = match provider {
        "sqlite" => r#"url      = "file:./dev.db""#,
        _ => r#"url      = env("DATABASE_URL")"#,
    };

    format!(
        r#"datasource db {{
  provider = "{provider}"
  {url}
}}

generator client {{
  output = "./src/generated"
}}

model User {{
  id        String   @id @default(uuid())
  email     String   @unique
  name      String?
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  @@map("users")
}}
"#
    )
}
