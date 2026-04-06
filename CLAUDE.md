# ormx - Prisma-Inspired ORM for Rust

## Architecture

6-crate workspace following onion architecture (dependency flows inward):

- **ormx-core**: Pure domain types (AST, Schema IR). ZERO external dependencies.
- **ormx-parser**: PEG grammar (pest) to parse `.ormx` schema files into AST, then validate into Schema IR.
- **ormx-codegen**: Generates type-safe Rust source files from Schema IR using `quote` + `prettyplease`.
- **ormx-runtime**: Ships with user's app. Database client, filter types, query builder, execution.
- **ormx-migrate**: Migration engine (schema diffing, SQL generation). Not yet implemented.
- **ormx-cli**: CLI binary (`ormx init`, `ormx generate`).

## Commands

```bash
cargo test --workspace          # Run all tests
cargo run -p ormx-cli -- --schema path/to/schema.ormx generate  # Generate code
```

## Key patterns

- All dependencies are defined at workspace level in root `Cargo.toml`
- Database backends (postgres, sqlite) are feature-flagged in ormx-runtime
- Generated code lives in user's project (e.g., `src/generated/`)
- Schema files use `.ormx` extension
- `examples/basic/` is excluded from the workspace and has its own Cargo.toml
