use std::fs;
use std::path::Path;

pub async fn run(schema_path: &str) -> miette::Result<()> {
    let schema_path = Path::new(schema_path);

    // Read schema file
    let source = fs::read_to_string(schema_path)
        .map_err(|e| miette::miette!("Failed to read {}: {e}", schema_path.display()))?;

    println!("Parsing schema...");

    // Parse and validate
    let schema = ormx_parser::parse_and_validate(&source)
        .map_err(|e| miette::miette!("Schema error: {e}"))?;

    // Determine output directory
    let output_dir = if let Some(generator) = schema.generators.first() {
        Path::new(&generator.output).to_path_buf()
    } else {
        Path::new("./src/generated").to_path_buf()
    };

    println!("Generating code to {}...", output_dir.display());

    // Generate code
    ormx_codegen::generator::generate(&schema, &output_dir)
        .map_err(|e| miette::miette!("Code generation failed: {e}"))?;

    // Count generated files
    let file_count = fs::read_dir(&output_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);

    println!(
        "Generated {file_count} files in {}",
        output_dir.display()
    );

    Ok(())
}
