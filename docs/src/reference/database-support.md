# Database Support

Ferriorm supports PostgreSQL and SQLite. Support is controlled by Cargo feature flags on the `ferriorm-runtime` crate.

## Feature Matrix

| Feature | PostgreSQL | SQLite |
|---|---|---|
| **Connection** | | |
| Connection pooling | Yes | Yes |
| PoolConfig | Yes | Yes |
| Auto-detection from URL | Yes | Yes |
| **CRUD** | | |
| create / find / update / delete | Yes | Yes |
| create_many / update_many / delete_many | Yes | Yes |
| count | Yes | Yes |
| **Filtering** | | |
| StringFilter (equals, contains, etc.) | Yes | Yes |
| IntFilter / BigIntFilter / FloatFilter | Yes | Yes |
| BoolFilter | Yes | Yes |
| DateTimeFilter | Yes | Yes |
| EnumFilter | Yes | Yes |
| AND / OR / NOT combinators | Yes | Yes |
| Case-insensitive mode (QueryMode) | Yes (ILIKE) | Partial (LIKE is case-insensitive for ASCII in SQLite) |
| **Ordering & Pagination** | | |
| ORDER BY | Yes | Yes |
| LIMIT / OFFSET | Yes | Yes |
| **Relations** | | |
| Include (batched loading) | Yes | Yes |
| Foreign keys | Yes | Yes (if enabled) |
| **Select** | | |
| Partial column selection | Yes | Yes |
| **Aggregates** | | |
| MIN / MAX | Yes | Yes |
| AVG / SUM | Yes | Yes |
| **Raw SQL** | | |
| raw_fetch_all / one / optional | Yes | Yes |
| raw_execute | Yes | Yes |
| Direct pool access | Yes (`pg_pool()`) | Yes (`sqlite_pool()`) |
| **Transactions** | | |
| run_transaction | Yes | Yes |
| Auto-rollback on error | Yes | Yes |
| **Migrations** | | |
| migrate dev (shadow DB) | Yes | Yes |
| migrate dev (snapshot) | Yes | Yes |
| migrate deploy | Yes | Yes |
| migrate status | Yes | Yes |
| db pull (introspection) | Yes | Yes |
| **Schema Features** | | |
| @id | Yes | Yes |
| @unique | Yes | Yes |
| @default(uuid()) | Yes | Yes |
| @default(now()) | Yes | Yes |
| @default(autoincrement()) | Yes | Yes |
| @updatedAt | Yes | Yes |
| @relation | Yes | Yes |
| @@index | Yes | Yes |
| @@unique (composite) | Yes | Yes |
| @@map | Yes | Yes |
| Enums (native) | Yes (CREATE TYPE) | Emulated (TEXT + CHECK) |

## URL Formats

### PostgreSQL

```
postgres://user:password@host:port/database
postgresql://user:password@host:port/database
```

Options can be appended as query parameters:

```
postgres://user:pass@host/db?sslmode=require
```

### SQLite

```
sqlite:path/to/database.db
sqlite::memory:
file:path/to/database.db
path/to/database.db
```

SQLite URLs support query parameters for pragmas:

```
sqlite:mydb.db?mode=rwc
```

## Type Mapping

### PostgreSQL

| Schema type | Rust type | PostgreSQL type |
|---|---|---|
| `String` | `String` | `TEXT` |
| `Int` | `i32` | `INTEGER` |
| `BigInt` | `i64` | `BIGINT` |
| `Float` | `f64` | `DOUBLE PRECISION` |
| `Boolean` | `bool` | `BOOLEAN` |
| `DateTime` | `chrono::DateTime<Utc>` | `TIMESTAMPTZ` |
| `enum Foo` | `Foo` (generated) | Custom enum type |

### SQLite

| Schema type | Rust type | SQLite type |
|---|---|---|
| `String` | `String` | `TEXT` |
| `Int` | `i32` | `INTEGER` |
| `BigInt` | `i64` | `INTEGER` |
| `Float` | `f64` | `REAL` |
| `Boolean` | `bool` | `BOOLEAN` |
| `DateTime` | `chrono::DateTime<Utc>` | `TEXT` (ISO 8601) |
| `enum Foo` | `Foo` (generated) | `TEXT` |

## SQLite DateTime Handling

SQLite's `CURRENT_TIMESTAMP` produces values in the format `YYYY-MM-DD HH:MM:SS` (without timezone). The sqlx chrono integration parses these values correctly, treating them as UTC. This means `@default(now())` works as expected on SQLite -- the value is stored as a TEXT column in ISO-like format and deserialized into `chrono::DateTime<Utc>` by sqlx.

Note that `@default(uuid())` and `@default(cuid())` do not produce SQL-level defaults on SQLite. The UUID/CUID values are generated in Rust application code before the INSERT statement is executed, so the column will always receive a value from the application.

## Known Limitations

| Limitation | Affects |
|---|---|
| No nested includes | Both |
| No raw SQL bind helpers (use sqlx directly) | Both |
| Enum columns stored as TEXT | SQLite |
| No array/JSON column types | Both |
| No database views | Both |
| No stored procedures | Both |
