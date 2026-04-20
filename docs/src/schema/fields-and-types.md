# Fields & Types

Every field in a model has a name and a type. This page covers all the scalar types supported by ferriorm, how they map to Rust and database types, and the type modifiers for optional and list fields.

## Scalar Types

ferriorm provides the following built-in scalar types:

| Schema Type | Rust Type | PostgreSQL | SQLite |
|---|---|---|---|
| `String` | `String` | `TEXT` | `TEXT` |
| `Int` | `i32` | `INTEGER` | `INTEGER` |
| `BigInt` | `i64` | `BIGINT` | `INTEGER` |
| `Float` | `f64` | `DOUBLE PRECISION` | `REAL` |
| `Decimal` | `rust_decimal::Decimal` | `DECIMAL` | `TEXT` |
| `Boolean` | `bool` | `BOOLEAN` | `INTEGER` |
| `DateTime` | `chrono::DateTime<chrono::Utc>` | `TIMESTAMPTZ` | `TEXT` |
| `Json` | `serde_json::Value` | `JSONB` | `TEXT` |
| `Bytes` | `Vec<u8>` | `BYTEA` | `BLOB` |

### `String`

Variable-length text. Use for names, emails, URLs, and other textual content.

```prisma
model User {
  name String
}
```

### `Int`

A 32-bit signed integer.

```prisma
model Product {
  quantity Int
}
```

### `BigInt`

A 64-bit signed integer. Use when values may exceed the 32-bit range.

```prisma
model Event {
  timestamp BigInt
}
```

### `Float`

A 64-bit floating-point number (double precision).

```prisma
model Measurement {
  value Float
}
```

### `Decimal`

An arbitrary-precision decimal number. Useful for monetary values or other cases where floating-point rounding is unacceptable.

```prisma
model Invoice {
  total Decimal
}
```

### `Boolean`

A true/false value. In SQLite, stored as `INTEGER` (0 or 1).

```prisma
model Post {
  published Boolean @default(false)
}
```

### `DateTime`

A timestamp with timezone. In Rust this is `chrono::DateTime<chrono::Utc>`. In SQLite, stored as an ISO 8601 `TEXT` string.

```prisma
model Post {
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt
}
```

### `Json`

Arbitrary JSON data. In Rust this is `serde_json::Value`. In PostgreSQL, stored as `JSONB` (binary JSON) for efficient querying. In SQLite, stored as `TEXT`.

```prisma
model Settings {
  data Json
}
```

### `Bytes`

Raw binary data. In Rust this is `Vec<u8>`.

```prisma
model File {
  content Bytes
}
```

## Type Modifiers

### Optional Fields (`?`)

Append `?` to a type to make the field optional. In the database, the column allows `NULL`. In Rust, the generated field type is wrapped in `Option<T>`.

```prisma
model User {
  name  String    // required -- Rust type: String
  bio   String?   // optional -- Rust type: Option<String>
}
```

A field without `?` is required -- it cannot be `NULL` in the database, and the Rust type does not use `Option`.

### List Fields (`[]`)

Append `[]` to a type to mark it as a list. This is used exclusively for **relation fields** to represent the "many" side of a one-to-many or many-to-many relationship. List fields are not stored as database columns; they are virtual fields resolved by querying the related table.

```prisma
model User {
  id    String @id
  posts Post[]    // one-to-many: a user has many posts
}

model Post {
  id       String @id
  author   User   @relation(fields: [authorId], references: [id])
  authorId String
}
```

See [Relations](./relations.md) for more details.

## Enum Fields

You can use any `enum` defined in your schema as a field type. The value must match one of the enum's variants.

```prisma
enum Role {
  User
  Admin
  Moderator
}

model User {
  id   String @id
  role Role   @default(User)
}
```

See [Enums](./enums.md) for details on how enums map to Rust and database types.

## Model Fields (Relations)

Using another model's name as a field type creates a relation. These fields are virtual -- they do not correspond to a database column directly. Instead, a separate foreign key field stores the actual value.

```prisma
model Post {
  id       String @id
  author   User   @relation(fields: [authorId], references: [id])
  authorId String   // this is the actual database column
}
```

See [Relations](./relations.md) for complete documentation.

## Native Database Type Overrides (`@db.Type`)

You can annotate fields with `@db.<Type>` to influence the generated Rust type and migration SQL. Currently the wired-up hint is:

```prisma
model Account {
  followerCount Int @default(0) @db.BigInt
}
```

`@db.BigInt` on an `Int` field generates `i64` (instead of `i32`) in Rust, uses `BigIntFilter`, and emits `BIGINT` in PostgreSQL migrations. SQLite's `INTEGER` is already variable-width so no migration change is needed there.

Other hints like `@db.VarChar(n)`, `@db.DoublePrecision`, and `@db.Text` parse successfully but are not yet consumed by code generation or migrations. See [Attributes › `@db.Type`](./attributes.md#dbtypeargs) for the full list.
