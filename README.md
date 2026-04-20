# ferriorm

**A Prisma-inspired ORM for Rust** -- schema-first, type-safe, with automatic code generation.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![CI](https://github.com/romanschejbal/ferriorm/actions/workflows/ci.yml/badge.svg)](https://github.com/romanschejbal/ferriorm/actions/workflows/ci.yml)

## Why ferriorm?

Existing Rust ORMs require manually defining structs, writing migrations, and wiring everything together. ferriorm takes the Prisma approach: **define your schema once**, and everything else -- type-safe Rust client, migrations, query builders -- is generated for you.

- **Schema-first**: Single `.ferriorm` file is your source of truth
- **Type-safe**: Generated Rust code catches errors at compile time
- **Zero boilerplate**: No derive macros to write, no manual struct definitions
- **Easy migrations**: Automatic schema diffing with shadow database support
- **Multi-database**: PostgreSQL and SQLite from day one

## Quick Start

### 1. Install

```bash
cargo install ferriorm-cli
```

### 2. Initialize

```bash
ferriorm init --provider postgresql
```

### 3. Define your schema

```prisma
// schema.ferriorm
datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

generator client {
  output = "./src/generated"
}

model User {
  id        String   @id @default(uuid())
  email     String   @unique
  name      String?
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  @@map("users")
}
```

### 4. Generate & migrate

```bash
ferriorm migrate dev --name init
```

### 5. Use in your code

```rust
use generated::FerriormClient;
use ferriorm_runtime::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = FerriormClient::connect("postgres://localhost/mydb").await?;

    // Create
    let user = client.user().create(user::data::UserCreateInput {
        email: "alice@example.com".into(),
        name: Some("Alice".into()),
    }).exec().await?;

    // Query with filters
    let users = client.user()
        .find_many(user::filter::UserWhereInput {
            email: Some(StringFilter {
                contains: Some("@example.com".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .order_by(user::order::UserOrderByInput::CreatedAt(SortOrder::Desc))
        .take(10)
        .exec().await?;

    // Relations with batched loading
    let users_with_posts = client.user()
        .find_many(user::filter::UserWhereInput::default())
        .include(user::UserInclude { posts: true, ..Default::default() })
        .exec().await?;

    Ok(())
}
```

## Features

### Schema Language
- Models with scalar fields, enums, relations
- Field attributes: `@id`, `@unique`, `@default`, `@updatedAt`, `@map`, `@relation`
- Block attributes: `@@map`, `@@index`, `@@unique`, `@@id`
- Datasource and generator configuration

### Generated Client
- Full CRUD: `create`, `find_unique`, `find_first`, `find_many`, `update`, `delete`, `upsert`
- Dedup-on-write: `create().on_conflict_ignore()` → `Option<T>`
- Race-safe updates: `update_first(WhereInput, data)` for compare-and-swap transitions
- Batch operations: `create_many`, `update_many`, `delete_many`
- Type-safe filters: `equals`, `not`, `contains`, `starts_with`, `gt`, `lt`, `in`, `AND`, `OR`, `NOT`
- Nullable filters with `IS NULL` / `IS NOT NULL` for every scalar type
- Compound `@@unique([...])` keys materialize as `WhereUniqueInput` variants (usable by `upsert` as `ON CONFLICT` targets)
- Ordering, pagination (`skip`/`take`), counting
- Relation loading via `include()` with batched queries (no N+1)

### Migrations
- **Shadow database** (default): Replays migrations on a temp DB, introspects, diffs -- handles manual SQL edits correctly
- **Snapshot mode** (`--snapshot`): Offline diffing via JSON snapshots
- Commands: `migrate dev`, `migrate deploy`, `migrate status`
- Editable SQL migration files with checksum verification

### Database Support
| Feature | PostgreSQL | SQLite |
|---------|-----------|--------|
| Query execution | yes | yes |
| Code generation | yes | yes |
| Migrations | yes | yes |
| Shadow database | yes | yes |
| Introspection | yes | yes |
| `db pull` | yes | yes |

## Architecture

ferriorm is a Rust workspace with 6 crates following onion architecture:

```
ferriorm-cli          -> orchestrates everything
|-- ferriorm-parser   -> parses .ferriorm schema files
|-- ferriorm-codegen  -> generates Rust source code
|-- ferriorm-migrate  -> migration engine
\-- ferriorm-runtime  -> ships with user's app (DB client, filters, queries)
      \-- ferriorm-core -> pure domain types (zero external dependencies)
```

## CLI Reference

| Command | Description |
|---------|-------------|
| `ferriorm init` | Initialize a new ferriorm project |
| `ferriorm generate` | Generate Rust client from schema |
| `ferriorm migrate dev` | Create + apply migration + regenerate (development) |
| `ferriorm migrate deploy` | Apply pending migrations (production) |
| `ferriorm migrate status` | Show migration status |
| `ferriorm db pull` | Introspect database and generate schema |

## Status

ferriorm is in active development. Here is what's done and what's planned:

### Done
- [x] Schema parser with PEG grammar
- [x] Code generator (models, enums, filters, CRUD, query builders)
- [x] Runtime with PostgreSQL and SQLite support
- [x] All CRUD operations with exec()
- [x] Type-safe filters with AND/OR/NOT
- [x] Ordering and pagination
- [x] Relation loading with batched queries (include API)
- [x] Migration engine with schema diffing
- [x] Shadow database migration strategy
- [x] Database introspection and `db pull`
- [x] PostgreSQL support
- [x] SQLite support
- [x] Connection pooling configuration
- [x] Raw SQL escape hatch (pool access + zero-bind helpers)
- [x] `select()` for partial column loading
- [x] Aggregate queries (avg, sum, min, max)

### Planned
- [ ] Compile-time query verification (hybrid sqlx approach)
- [ ] MySQL support
- [ ] Middleware/hooks (beforeCreate, afterUpdate)
- [ ] Soft deletes
- [ ] Aggregate groupBy
- [ ] Cursor-based pagination
- [ ] Schema formatting (`ferriorm format`)
- [ ] LSP for .ferriorm files
- [ ] Seeding support

## License

MIT OR Apache-2.0
