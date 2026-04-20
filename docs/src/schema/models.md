# Models

A `model` block represents a database table and its columns. Each model produces a Rust struct with typed fields, along with query builder methods for CRUD operations.

## Syntax

```prisma
model User {
  id    String @id @default(uuid())
  email String @unique
  name  String?
  posts Post[]

  @@map("users")
  @@index([email])
}
```

## Naming Conventions

The model name must be written in **PascalCase** (e.g., `User`, `BlogPost`, `OrderItem`).

By default, ferriorm derives the database table name by converting the PascalCase model name to **snake_case** and appending **"s"**:

| Model Name | Default Table Name |
|---|---|
| `User` | `users` |
| `BlogPost` | `blog_posts` |
| `OrderItem` | `order_items` |

To override the default table name, use the `@@map` block attribute:

```prisma
model User {
  id String @id

  @@map("app_users")
}
```

This maps the `User` model to the `app_users` table instead of the default `users`.

## Fields

Each line inside a model body defines a field. A field consists of:

1. **Name** -- a camelCase identifier (e.g., `email`, `createdAt`)
2. **Type** -- a scalar type, enum name, or model name, with optional `?` or `[]` modifier
3. **Attributes** -- zero or more field-level attributes (e.g., `@id`, `@unique`, `@default(...)`)

```prisma
model Post {
  id        String     @id @default(uuid())
  title     String
  content   String?
  published Boolean    @default(false)
  status    PostStatus @default(Draft)
  author    User       @relation(fields: [authorId], references: [id])
  authorId  String
  createdAt DateTime   @default(now())
  updatedAt DateTime   @updatedAt
}
```

See [Fields & Types](./fields-and-types.md) and [Attributes](./attributes.md) for full details on types and attributes.

## Block Attributes

Block attributes appear at the bottom of a model body and are prefixed with `@@`. They apply to the model (table) as a whole rather than to individual fields.

### `@@map`

Override the default table name.

```prisma
model User {
  id String @id

  @@map("users")
}
```

### `@@index`

Create a database index on one or more fields. This improves query performance for lookups and sorts on the indexed columns.

```prisma
model Post {
  id       String @id
  authorId String
  title    String

  @@index([authorId])
  @@index([authorId, title])
}
```

### `@@unique`

Create a composite unique constraint across multiple fields. This ensures that the combination of values is unique across all rows. Each `@@unique` also becomes a struct-style variant on the generated `WhereUniqueInput`, so it can be used for `find_unique`, `update`, `delete`, and as the `upsert` conflict target. See [Attributes › `@@unique`](./attributes.md#unique-field1-field2-) for the generated shape.

```prisma
model Subscription {
  id      String @id
  userId  Int
  channel String

  @@unique([userId, channel])
}
```

> For single-field uniqueness, prefer the `@unique` field attribute instead.

### `@@id`

Define a composite primary key. Use this when the primary key spans more than one field.

```prisma
model PostTag {
  postId String
  tagId  String

  @@id([postId, tagId])
}
```

When using `@@id`, no individual field needs the `@id` attribute.

## Complete Example

```prisma
model User {
  id        String   @id @default(uuid())
  email     String   @unique
  name      String?
  role      Role     @default(User)
  posts     Post[]
  profile   Profile?
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  @@index([email])
  @@map("users")
}
```

This model:

- Uses a UUID string as the primary key, auto-generated on insert
- Enforces a unique constraint on `email`
- Makes `name` optional (`String?` maps to `Option<String>` in Rust)
- Stores a `Role` enum value, defaulting to `User`
- Has a one-to-many relation to `Post` (the `posts` field) and a one-to-one relation to `Profile` (the `profile` field)
- Tracks creation and update timestamps automatically
- Creates a database index on the `email` column
- Maps to a table named `users`
