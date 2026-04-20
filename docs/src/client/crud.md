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

`UserWhereUniqueInput` is an enum with one variant per `@unique` or `@id` field, plus a **struct-style variant for every `@@unique([...])` on the model**.

```prisma
model Subscription {
  id      String @id
  userId  Int
  channel String

  @@unique([userId, channel])
}
```

```rust
use generated::subscription::filter::SubscriptionWhereUniqueInput;

client.subscription()
    .find_unique(SubscriptionWhereUniqueInput::UserIdChannel {
        user_id: 42,
        channel: "ig".into(),
    })
    .exec()
    .await?;
```

The same compound variant is accepted by `update`, `delete`, and `upsert`.

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

Insert a record or update it if the conflict target already exists. Uses `INSERT ... ON CONFLICT DO UPDATE` under the hood — works on both PostgreSQL and SQLite.

The **conflict target is derived at runtime from the `WhereUniqueInput` variant** you pass: a single-field variant (`::Id(..)`, `::Email(..)`) targets that column, a compound variant (`::UserIdChannel { .. }`) targets all its columns. This lets a single `upsert` cover every `@unique` and `@@unique` on the model.

```rust
// Upsert by a single unique field:
let user = client.user().upsert(
    user::filter::UserWhereUniqueInput::Email("alice@example.com".into()),
    user::data::UserCreateInput {
        email: "alice@example.com".into(),
        name: Some("Alice".into()),
        role: None,
        id: None,
        created_at: None,
    },
    user::data::UserUpdateInput {
        name: Some(SetValue::Set(Some("Alice Updated".into()))),
        ..Default::default()
    },
).exec().await?;

// Upsert by a compound @@unique([userId, channel]):
client.subscription().upsert(
    subscription::filter::SubscriptionWhereUniqueInput::UserIdChannel {
        user_id: 42,
        channel: "ig".into(),
    },
    create_input,
    update_input,
).exec().await?;
```

If no update fields are provided (`UpdateInput::default()`), the existing row is returned unchanged.

## Create with On-Conflict Ignore

Dedup-on-write: insert the record, or silently skip it if a unique constraint already holds. Returns `Ok(None)` when the insert was suppressed, `Ok(Some(row))` when it succeeded.

```rust
let maybe_event = client
    .webhook_event()
    .create(WebhookEventCreateInput {
        external_id: "evt_abc123".into(),
        payload: Some(body),
        id: None,
        created_at: None,
    })
    .on_conflict_ignore()
    .exec()
    .await?;

match maybe_event {
    Some(row) => println!("stored new event {}", row.id),
    None => println!("duplicate event, ignored"),
}
```

Under the hood: PostgreSQL emits `ON CONFLICT DO NOTHING RETURNING *`, SQLite emits `INSERT OR IGNORE ... RETURNING *`. No conflict target is specified, so any unique violation (primary key, single `@unique`, or `@@unique`) triggers the ignore path.

## Update First (compare-and-swap)

`update` only accepts a `WhereUniqueInput`, which means the row is located solely by its unique key. For state-machine transitions (`status = 'pending' → 'approved'`) you usually want extra predicates so the update is race-safe:

```sql
UPDATE submissions SET status = 'approved' WHERE id = ? AND status = 'pending' RETURNING *;
```

Use `update_first` for that. It takes a full `WhereInput` (same type as `find_first`/`update_many`) and returns `Result<Option<Model>>` — `None` if no row matched.

```rust
let approved = client
    .submission()
    .update_first(
        submission::filter::SubmissionWhereInput {
            id: Some(StringFilter { equals: Some(id.clone()), ..Default::default() }),
            status: Some(EnumFilter { equals: Some(Status::Pending), ..Default::default() }),
            ..Default::default()
        },
        submission::data::SubmissionUpdateInput {
            status: Some(SetValue::Set(Status::Approved)),
            ..Default::default()
        },
    )
    .exec()
    .await?;

if approved.is_none() {
    // Another concurrent worker already moved it out of `pending`.
}
```

Unlike `update_many`, `update_first` returns the updated row. Narrow the filter to one row (typically by including the primary key) — if multiple rows match, all of them are updated but only the first is returned.

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
