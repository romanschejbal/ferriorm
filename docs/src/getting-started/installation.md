# Installation

ferriorm has two parts: a **CLI** that parses your schema and generates code, and a **runtime library** that ships with your application.

## 1. Install the CLI

```bash
cargo install ferriorm-cli
```

This gives you the `ferriorm` command. Verify it works:

```bash
ferriorm --version
```

## 2. Add runtime dependencies

Add the ferriorm runtime to your project. Pick the feature flag for your database:

```bash
# PostgreSQL
cargo add ferriorm-runtime --features postgres

# SQLite
cargo add ferriorm-runtime --features sqlite

# Both
cargo add ferriorm-runtime --features postgres,sqlite
```

## 3. Add required companion crates

ferriorm's generated code depends on a few standard crates. Add them to your project.

> **Important:** `sqlx` must be a **direct** dependency in your `Cargo.toml`. The generated code uses `#[derive(sqlx::FromRow)]`, which is a proc macro that expands to code referencing `::sqlx::` absolute paths internally. Transitive dependencies (through `ferriorm-runtime`) are not sufficient for absolute path resolution in Rust.

```bash
cargo add sqlx --features runtime-tokio,tls-rustls,postgres,chrono,uuid
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json
cargo add chrono --features serde
cargo add uuid --features v4,serde
```

> For SQLite, replace `postgres` with `sqlite` in the sqlx features.

## Complete Cargo.toml example

Here is a full `Cargo.toml` for a PostgreSQL project:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"

[dependencies]
ferriorm-runtime = { version = "0.1", features = ["postgres"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "uuid"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
```

And for SQLite:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"

[dependencies]
ferriorm-runtime = { version = "0.1", features = ["sqlite"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "sqlite", "chrono", "uuid"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
```

## Next steps

With everything installed, head to the [Quick Start](./quick-start.md) guide to create your first ferriorm project.
