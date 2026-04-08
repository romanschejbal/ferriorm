# CRUD Operations

All CRUD operations follow the same pattern: call a method on the model accessor, optionally chain modifiers, then call `.exec().await?` to execute.

```rust
let user = client.user().create(input).exec().await?;
```

The examples below assume a `User` model with fields `id`, `email`, `name`, `role`, `createdAt`, and `updatedAt`.

## Create

Insert a single record. Returns the created record with all server-generated fields populated.

```rust
use generated::user::data::UserCreateInput;
use generated::Role;

let user = client
    .user()
    .create(UserCreateInput {
        email: "alice@example.com".into(),
        name: Some("Alice".into()),
        role: Some(Role::Admin),
        id: None,        // auto-generated (uuid)
        created_at: None, // auto-generated (now)
    })
    .exec()
    .await?;

println!("Created: {} (id={})", user.email, user.id);
```

**Required vs optional fields:**

| Field kind | `CreateInput` type | Notes |
|---|---|---|
| Required, no default | `T` | Must be provided |
| Optional (`?` in schema) | `Option<T>` | `None` inserts NULL |
| Has `@default(...)` | `Option<T>` | `None` uses the default |
| `@id @default(uuid())` | `Option<String>` | `None` auto-generates a UUID |
| `@default(now())` | `Option<DateTime>` | `None` uses current timestamp |

## Find Unique

Fetch a single record by a unique field. Returns `Option<Model>`.

```rust
use generated::user::filter::UserWhereUniqueInput;

// By ID
let user = client
    .user()
    .find_unique(UserWhereUniqueInput::Id("some-uuid".into()))
    .exec()
    .await?;

// By unique field
let user = client
    .user()
    .find_unique(UserWhereUniqueInput::Email("alice@example.com".into()))
    .exec()
    .await?;

if let Some(u) = user {
    println!("Found: {}", u.email);
}
```

`UserWhereUniqueInput` is an enum with one variant per `@unique` or `@id` field.

## Find First

Fetch the first matching record, with optional ordering. Returns `Option<Model>`.

```rust
use generated::user::filter::UserWhereInput;
use generated::user::order::UserOrderByInput;
use ferriorm_runtime::prelude::*;

let newest = client
    .user()
    .find_first(UserWhereInput {
        email: Some(StringFilter {
            contains: Some("@example.com".into()),
            ..Default::default()
        }),
        ..Default::default()
    })
    .order_by(UserOrderByInput::CreatedAt(SortOrder::Desc))
    .exec()
    .await?;
```

## Find Many

Fetch multiple records with filtering, ordering, and pagination.

```rust
let users = client
    .user()
    .find_many(UserWhereInput::default()) // no filter = all records
    .order_by(UserOrderByInput::CreatedAt(SortOrder::Desc))
    .skip(0)
    .take(10)
    .exec()
    .await?;
```

Returns `Vec<Model>`. An empty `Vec` when no records match (never errors for zero results).

## Update

Update a single record by unique field. Returns the updated record.

```rust
use generated::user::data::UserUpdateInput;
use generated::user::filter::UserWhereUniqueInput;
use ferriorm_runtime::prelude::*;

let updated = client
    .user()
    .update(
        UserWhereUniqueInput::Id("some-uuid".into()),
        UserUpdateInput {
            name: Some(SetValue::Set(Some("Alice Smith".into()))),
            role: Some(SetValue::Set(Role::Moderator)),
            ..Default::default()
        },
    )
    .exec()
    .await?;
```

**`SetValue` wrapper:** Update fields use `Option<SetValue<T>>`:

- `None` -- field is not modified
- `Some(SetValue::Set(value))` -- set the field to `value`

For nullable fields, the inner type is `Option<T>`, so setting a field to NULL looks like `Some(SetValue::Set(None))`.

Fields with `@updatedAt` are automatically set to the current timestamp on every update.

## Delete

Delete a single record by unique field. Returns the deleted record.

```rust
let deleted = client
    .user()
    .delete(UserWhereUniqueInput::Id("some-uuid".into()))
    .exec()
    .await?;

println!("Deleted: {}", deleted.email);
```

## Upsert

Insert a record or update it if it already exists (based on the primary key). Uses `INSERT ... ON CONFLICT DO UPDATE` under the hood — works on both PostgreSQL and SQLite.

```rust
let user = client.user().upsert(
    // Which unique field to match on:
    user::filter::UserWhereUniqueInput::Email("alice@example.com".into()),
    // Data to INSERT if not found:
    user::data::UserCreateInput {
        email: "alice@example.com".into(),
        name: Some("Alice".into()),
        role: None,
        id: None,
        created_at: None,
    },
    // Data to UPDATE if found:
    user::data::UserUpdateInput {
        name: Some(SetValue::Set(Some("Alice Updated".into()))),
        ..Default::default()
    },
).exec().await?;
```

If no update fields are provided (`UpdateInput::default()`), the existing row is returned unchanged.

## Create Many

Insert multiple records in a batch. Returns the number of records created.

```rust
let count = client
    .user()
    .create_many(vec![
        UserCreateInput {
            email: "bob@example.com".into(),
            name: Some("Bob".into()),
            role: None,
            id: None,
            created_at: None,
        },
        UserCreateInput {
            email: "carol@example.com".into(),
            name: Some("Carol".into()),
            role: None,
            id: None,
            created_at: None,
        },
    ])
    .exec()
    .await?;

println!("Created {count} users");
```

## Update Many

Update all records matching a filter. Returns the number of rows affected.

```rust
let count = client
    .user()
    .update_many(
        UserWhereInput {
            role: Some(EnumFilter {
                equals: Some(Role::User),
                ..Default::default()
            }),
            ..Default::default()
        },
        UserUpdateInput {
            role: Some(SetValue::Set(Role::Moderator)),
            ..Default::default()
        },
    )
    .exec()
    .await?;

println!("Updated {count} users");
```

## Delete Many

Delete all records matching a filter. Returns the number of rows deleted.

```rust
let count = client
    .user()
    .delete_many(UserWhereInput {
        role: Some(EnumFilter {
            equals: Some(Role::Admin),
            ..Default::default()
        }),
        ..Default::default()
    })
    .exec()
    .await?;

println!("Deleted {count} users");
```

Pass `UserWhereInput::default()` to delete **all** records (use with caution).

## Count

Count records matching a filter. Returns `i64`.

```rust
let total = client
    .user()
    .count(UserWhereInput::default())
    .exec()
    .await?;

println!("Total users: {total}");
```

With a filter:

```rust
let admin_count = client
    .user()
    .count(UserWhereInput {
        role: Some(EnumFilter {
            equals: Some(Role::Admin),
            ..Default::default()
        }),
        ..Default::default()
    })
    .exec()
    .await?;
```
