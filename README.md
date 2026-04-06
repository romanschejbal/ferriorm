# ormx

**A Prisma-inspired ORM for Rust** -- schema-first, type-safe, with automatic code generation.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![CI](https://github.com/romanschejbal/ormx/actions/workflows/ci.yml/badge.svg)](https://github.com/romanschejbal/ormx/actions/workflows/ci.yml)

## Why ormx?

Existing Rust ORMs require manually defining structs, writing migrations, and wiring everything together. ormx takes the Prisma approach: **define your schema once**, and everything else -- type-safe Rust client, migrations, query builders -- is generated for you.

- **Schema-first**: Single `.ormx` file is your source of truth
- **Type-safe**: Generated Rust code catches errors at compile time
- **Zero boilerplate**: No derive macros to write, no manual struct definitions
- **Easy migrations**: Automatic schema diffing with shadow database support
- **Multi-database**: PostgreSQL and SQLite from day one

## Quick Start

### 1. Install

```bash
cargo install ormx-cli
```

### 2. Initialize

```bash
ormx init --provider postgresql
```

### 3. Define your schema

```prisma
// schema.ormx
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
ormx migrate dev --name init
```

### 5. Use in your code

```rust
use generated::OrmxClient;
use ormx_runtime::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = OrmxClient::connect("postgres://localhost/mydb").await?;

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
- Batch operations: `create_many`, `update_many`, `delete_many`
- Type-safe filters: `equals`, `not`, `contains`, `starts_with`, `gt`, `lt`, `in`, `AND`, `OR`, `NOT`
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

ormx is a Rust workspace with 6 crates following onion architecture:

```
ormx-cli          -> orchestrates everything
|-- ormx-parser   -> parses .ormx schema files
|-- ormx-codegen  -> generates Rust source code
|-- ormx-migrate  -> migration engine
\-- ormx-runtime  -> ships with user's app (DB client, filters, queries)
      \-- ormx-core -> pure domain types (zero external dependencies)
```

## CLI Reference

| Command | Description |
|---------|-------------|
| `ormx init` | Initialize a new ormx project |
| `ormx generate` | Generate Rust client from schema |
| `ormx migrate dev` | Create + apply migration + regenerate (development) |
| `ormx migrate deploy` | Apply pending migrations (production) |
| `ormx migrate status` | Show migration status |
| `ormx db pull` | Introspect database and generate schema |

## Status

ormx is in active development. Here is what's done and what's planned:

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

### Planned
- [ ] Compile-time query verification (hybrid sqlx approach)
- [ ] MySQL support
- [ ] `select()` for partial column loading
- [ ] Middleware/hooks (beforeCreate, afterUpdate)
- [ ] Soft deletes
- [ ] Raw SQL escape hatch
- [ ] Aggregate queries (sum, avg, min, max, groupBy)
- [ ] Cursor-based pagination
- [ ] Schema formatting (`ormx format`)
- [ ] LSP for .ormx files
- [ ] Connection pooling configuration
- [ ] Seeding support

## License

MIT OR Apache-2.0
