mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ormx", about = "Prisma-like ORM for Rust", version)]
struct Cli {
    /// Path to the schema file
    #[arg(long, default_value = "schema.ormx")]
    schema: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new ormx project
    Init {
        /// Database provider (postgresql, sqlite)
        #[arg(long, default_value = "postgresql")]
        provider: String,
    },

    /// Generate the Rust client from the schema
    Generate,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { provider } => commands::init::run(&provider).await,
        Commands::Generate => commands::generate::run(&cli.schema).await,
    }
}
