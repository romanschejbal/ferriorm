# Filtering

Filters are type-safe structs that map to SQL `WHERE` clauses. Each scalar type in your schema has a corresponding filter type. Filters are used with `find_many`, `find_first`, `update_many`, `delete_many`, and `count`.

## WhereInput Structure

Every model generates a `WhereInput` struct. Each field is `Option<FilterType>` -- set it to apply that condition, leave it `None` to skip.

```rust
use generated::user::filter::UserWhereInput;
use ferriorm_runtime::filter::StringFilter;

let filter = UserWhereInput {
    email: Some(StringFilter {
        contains: Some("@example.com".into()),
        ..Default::default()
    }),
    ..Default::default()
};
```

Multiple fields set on the same `WhereInput` are combined with `AND`.

## StringFilter

For `String` fields.

```rust
use ferriorm_runtime::filter::StringFilter;

// Exact match
StringFilter { equals: Some("alice@example.com".into()), ..Default::default() }

// Not equal
StringFilter { not: Some("bob@example.com".into()), ..Default::default() }

// Contains substring (SQL LIKE '%value%')
StringFilter { contains: Some("example".into()), ..Default::default() }

// Starts with (SQL LIKE 'value%')
StringFilter { starts_with: Some("alice".into()), ..Default::default() }

// Ends with (SQL LIKE '%value')
StringFilter { ends_with: Some("@example.com".into()), ..Default::default() }

// In a list of values
StringFilter { r#in: Some(vec!["a@ex.com".into(), "b@ex.com".into()]), ..Default::default() }

// Not in a list
StringFilter { not_in: Some(vec!["spam@ex.com".into()]), ..Default::default() }
```

### Case-Insensitive Mode

Set `mode: Some(QueryMode::Insensitive)` for case-insensitive string matching:

```rust
use ferriorm_runtime::filter::{StringFilter, QueryMode};

StringFilter {
    contains: Some("alice".into()),
    mode: Some(QueryMode::Insensitive),
    ..Default::default()
}
```

### Nullable filters (`IS NULL` / `IS NOT NULL`)

Every optional (`?`) scalar field gets a matching nullable filter: `NullableStringFilter`, `NullableIntFilter`, `NullableBigIntFilter`, `NullableFloatFilter`, `NullableBoolFilter`, `NullableDateTimeFilter`.

They behave like the non-nullable siblings except `equals` and `not` are `Option<Option<T>>`:

- `None` — the condition is not applied.
- `Some(None)` — matches NULL via SQL `IS NULL` (or `IS NOT NULL` for `not`).
- `Some(Some(v))` — ordinary equality/inequality against `v`.

```rust
use ferriorm_runtime::filter::{NullableStringFilter, NullableDateTimeFilter};

// Users with no name set (name IS NULL)
NullableStringFilter { equals: Some(None), ..Default::default() }

// A specific name
NullableStringFilter { equals: Some(Some("Alice".into())), ..Default::default() }

// Profiles that have not been disconnected yet (disconnected_at IS NULL)
NullableDateTimeFilter { equals: Some(None), ..Default::default() }

// Anything that HAS been disconnected
NullableDateTimeFilter { not: Some(None), ..Default::default() }
```

Ordering operators (`gt`, `gte`, `lt`, `lte`) and `contains`/`starts_with`/`ends_with` keep their non-nullable `Option<T>` shape — they intentionally can't express `IS NULL`, and NULL never matches `LIKE` or comparison operators anyway.

## IntFilter

For `Int` (`i32`) fields.

```rust
use ferriorm_runtime::filter::IntFilter;

// Exact match
IntFilter { equals: Some(42), ..Default::default() }

// Not equal
IntFilter { not: Some(0), ..Default::default() }

// Greater than
IntFilter { gt: Some(18), ..Default::default() }

// Greater than or equal
IntFilter { gte: Some(18), ..Default::default() }

// Less than
IntFilter { lt: Some(100), ..Default::default() }

// Less than or equal
IntFilter { lte: Some(100), ..Default::default() }

// In a list
IntFilter { r#in: Some(vec![1, 2, 3]), ..Default::default() }

// Not in a list
IntFilter { not_in: Some(vec![0, -1]), ..Default::default() }
```

### Range Example

Combine operators to create ranges:

```rust
IntFilter {
    gte: Some(18),
    lt: Some(65),
    ..Default::default()
}
```

## BigIntFilter

For `BigInt` (`i64`) fields. Same operators as `IntFilter`.

## FloatFilter

For `Float` (`f64`) fields. Supports `equals`, `not`, `gt`, `gte`, `lt`, `lte`.

## BoolFilter

For `Boolean` fields.

```rust
use ferriorm_runtime::filter::BoolFilter;

// Match published posts
BoolFilter { equals: Some(true), ..Default::default() }

// Match unpublished posts
BoolFilter { not: Some(true), ..Default::default() }
```

## DateTimeFilter

For `DateTime` fields. Uses `chrono::DateTime<chrono::Utc>`.

```rust
use ferriorm_runtime::filter::DateTimeFilter;
use chrono::{Utc, Duration};

// Created in the last 24 hours
DateTimeFilter {
    gt: Some(Utc::now() - Duration::hours(24)),
    ..Default::default()
}

// Created before a specific date
DateTimeFilter {
    lt: Some("2025-01-01T00:00:00Z".parse().unwrap()),
    ..Default::default()
}

// Exact match
DateTimeFilter { equals: Some(some_datetime), ..Default::default() }

// In a list
DateTimeFilter { r#in: Some(vec![date1, date2]), ..Default::default() }
```

Supported operators: `equals`, `not`, `gt`, `gte`, `lt`, `lte`, `in`.

## EnumFilter

For enum fields. Generic over the enum type.

```rust
use ferriorm_runtime::filter::EnumFilter;
use generated::Role;

// Exact match
EnumFilter { equals: Some(Role::Admin), ..Default::default() }

// Not equal
EnumFilter { not: Some(Role::User), ..Default::default() }

// In a list
EnumFilter {
    r#in: Some(vec![Role::Admin, Role::Moderator]),
    ..Default::default()
}

// Not in a list
EnumFilter {
    not_in: Some(vec![Role::User]),
    ..Default::default()
}
```

## AND / OR / NOT Combinators

The `WhereInput` struct includes `and`, `or`, and `not` fields for composing complex conditions.

### AND

All conditions must match. This is the default when setting multiple fields, but `and` lets you express the same field with different conditions:

```rust
UserWhereInput {
    and: Some(vec![
        UserWhereInput {
            email: Some(StringFilter {
                contains: Some("example".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        UserWhereInput {
            name: Some(NullableStringFilter {
                not: Some(None), // name IS NOT NULL
                ..Default::default()
            }),
            ..Default::default()
        },
    ]),
    ..Default::default()
}
```

### OR

At least one condition must match:

```rust
UserWhereInput {
    or: Some(vec![
        UserWhereInput {
            role: Some(EnumFilter { equals: Some(Role::Admin), ..Default::default() }),
            ..Default::default()
        },
        UserWhereInput {
            role: Some(EnumFilter { equals: Some(Role::Moderator), ..Default::default() }),
            ..Default::default()
        },
    ]),
    ..Default::default()
}
```

### NOT

Negate a condition:

```rust
UserWhereInput {
    not: Some(Box::new(UserWhereInput {
        email: Some(StringFilter {
            ends_with: Some("@spam.com".into()),
            ..Default::default()
        }),
        ..Default::default()
    })),
    ..Default::default()
}
```

## Complete Example

Find active admin users created in the last week whose email is from a specific domain:

```rust
use chrono::{Utc, Duration};

let users = client
    .user()
    .find_many(UserWhereInput {
        role: Some(EnumFilter {
            equals: Some(Role::Admin),
            ..Default::default()
        }),
        email: Some(StringFilter {
            ends_with: Some("@company.com".into()),
            ..Default::default()
        }),
        created_at: Some(DateTimeFilter {
            gte: Some(Utc::now() - Duration::weeks(1)),
            ..Default::default()
        }),
        ..Default::default()
    })
    .exec()
    .await?;
```
