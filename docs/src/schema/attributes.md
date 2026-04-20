# Attributes

Attributes annotate fields and models with additional behavior such as primary keys, defaults, uniqueness constraints, and relations. There are two kinds:

- **Field attributes** (prefixed with `@`) apply to a single field.
- **Block attributes** (prefixed with `@@`) apply to the model as a whole.

---

## Field Attributes

### `@id`

Marks a field as the **primary key** of the model.

```prisma
model User {
  id String @id
}
```

Every model must have either a field with `@id` or a `@@id` block attribute for composite primary keys.

Common patterns:

```prisma
// UUID primary key (generated automatically)
id String @id @default(uuid())

// CUID primary key
id String @id @default(cuid())

// Auto-incrementing integer primary key
id Int @id @default(autoincrement())
```

---

### `@unique`

Adds a **unique constraint** to the field. The database will reject any insert or update that would create a duplicate value in this column.

```prisma
model User {
  id    String @id
  email String @unique
}
```

For composite uniqueness across multiple fields, use `@@unique` instead.

---

### `@default(value)`

Sets a **default value** for the field. When a record is created without specifying this field, the default is used.

The argument can be one of the following:

#### Functions

| Function | Description | Applicable Types |
|---|---|---|
| `uuid()` | Generates a random UUID v4 | `String` |
| `cuid()` | Generates a CUID | `String` |
| `autoincrement()` | Auto-incrementing integer | `Int`, `BigInt` |
| `now()` | Current date and time | `DateTime` |

```prisma
model User {
  id        String   @id @default(uuid())
  createdAt DateTime @default(now())
}

model Counter {
  id Int @id @default(autoincrement())
}
```

#### Literal values

You can use string, integer, float, or boolean literals as defaults.

```prisma
model Post {
  published Boolean @default(false)
  views     Int     @default(0)
  category  String  @default("general")
}
```

#### Enum variants

When a field's type is an enum, the default value is the variant name (without quotes).

```prisma
enum Role {
  User
  Admin
}

model User {
  id   String @id
  role Role   @default(User)
}
```

---

### `@updatedAt`

Automatically sets the field to the **current timestamp** whenever the record is updated. Applicable to `DateTime` fields only.

```prisma
model Post {
  id        String   @id
  updatedAt DateTime @updatedAt
}
```

---

### `@map("column_name")`

Overrides the database **column name** for a field. By default, the column name matches the field name. Use `@map` when the database column follows a different naming convention.

```prisma
model User {
  id        String   @id
  createdAt DateTime @default(now()) @map("created_at")
  updatedAt DateTime @updatedAt      @map("updated_at")
}
```

In this example, the Rust struct field is `created_at` (derived from `createdAt`), but the database column is explicitly named `created_at`.

---

### `@relation(fields: [...], references: [...], onDelete: ..., onUpdate: ...)`

Defines a **relation** between two models. This attribute goes on the field that represents the related model.

```prisma
model Post {
  id       String @id
  author   User   @relation(fields: [authorId], references: [id])
  authorId String
}
```

#### Arguments

| Argument | Required | Description |
|---|---|---|
| `fields` | Yes | Array of field names on _this_ model that store the foreign key |
| `references` | Yes | Array of field names on the _related_ model that the foreign key points to |
| `onDelete` | No | Referential action when the referenced record is deleted |
| `onUpdate` | No | Referential action when the referenced record's key is updated |

#### Referential actions

| Action | Description |
|---|---|
| `Cascade` | Delete/update all related records |
| `Restrict` | Prevent deletion/update if related records exist |
| `NoAction` | Similar to Restrict (database-dependent) |
| `SetNull` | Set the foreign key field(s) to `NULL` |
| `SetDefault` | Set the foreign key field(s) to their default value |

```prisma
model Post {
  id       String @id
  author   User   @relation(fields: [authorId], references: [id], onDelete: Cascade)
  authorId String
}
```

See [Relations](./relations.md) for complete examples.

---

### `@db.Type(args)`

Specifies a **native database type** override for the column, bypassing the default type mapping.

```prisma
model Product {
  id    String @id
  name  String @db.VarChar(255)
  price Float  @db.DoublePrecision
}
```

The type name after `@db.` and any arguments in parentheses are parsed from the schema. Currently, the fully-wired hint is:

| Hint | Effect |
|---|---|
| `@db.BigInt` on `Int` | Generates `i64` (not `i32`) in Rust, uses `BigIntFilter`, migrates as `BIGINT` on PostgreSQL. SQLite's `INTEGER` is already variable-width, so no migration change is needed there. |

Use this when your `Int` column holds values that can exceed `i32` range — follower counts, byte sizes, aggregate sums — without committing the whole schema to `BigInt`.

```prisma
model Account {
  id            String @id
  followerCount Int    @default(0) @db.BigInt
  storageBytes  Int    @default(0) @db.BigInt
}
```

Other hints like `@db.VarChar(255)` and `@db.Decimal(p, s)` parse but are not yet consumed by code generation or migrations.

---

## Block Attributes

Block attributes appear at the bottom of a model body (after all field definitions) and are prefixed with `@@`.

### `@@map("table_name")`

Overrides the **database table name** for the model.

```prisma
model User {
  id String @id

  @@map("app_users")
}
```

Without `@@map`, the table name is derived automatically from the model name (PascalCase to snake_case + "s").

---

### `@@index([field1, field2, ...])`

Creates a **database index** on one or more fields. Indexes speed up queries that filter or sort by the indexed columns.

```prisma
model Post {
  id       String @id
  authorId String
  title    String

  @@index([authorId])
}
```

Composite indexes span multiple columns:

```prisma
model Post {
  id        String   @id
  authorId  String
  createdAt DateTime

  @@index([authorId, createdAt])
}
```

You can define multiple `@@index` attributes on the same model.

---

### `@@unique([field1, field2, ...])`

Creates a **composite unique constraint** across multiple fields. The database will reject any insert or update that would create a duplicate combination of values in these columns.

```prisma
model Subscription {
  id      String @id
  userId  Int
  channel String

  @@unique([userId, channel])
}
```

Each `@@unique` also materializes as a struct-style variant on the model's `WhereUniqueInput` enum, so you can use it with `find_unique`, `update`, `delete`, and `upsert`. The variant name is the PascalCase concatenation of the field names:

```rust
use generated::subscription::filter::SubscriptionWhereUniqueInput;

client.subscription().upsert(
    SubscriptionWhereUniqueInput::UserIdChannel {
        user_id: 42,
        channel: "ig".into(),
    },
    create_input,
    update_input,
).exec().await?;
```

At the SQL layer, `upsert` uses this compound key as the `ON CONFLICT (...)` target automatically.

> For single-field uniqueness, prefer the `@unique` field attribute.

---

### `@@id([field1, field2, ...])`

Defines a **composite primary key** spanning multiple fields. When using `@@id`, individual fields should not carry the `@id` attribute.

```prisma
model PostTag {
  postId String
  tagId  String

  @@id([postId, tagId])
}
```

This creates a table where the combination of `postId` and `tagId` forms the primary key.
