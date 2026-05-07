# Aggregates

Ferriorm supports aggregate operations (`min`, `max`, `avg`, `sum`) on model fields. The operations are type-checked at compile time based on the field's data type.

## Basic Usage

```rust
use generated::user::UserAggregateField;
use generated::user::filter::UserWhereInput;

let result = client
    .user()
    .aggregate(UserWhereInput::default())
    .min(UserAggregateField::CreatedAt)
    .max(UserAggregateField::CreatedAt)
    .exec()
    .await?;

println!("Earliest user: {:?}", result.min_created_at);
println!("Latest user: {:?}", result.max_created_at);
```

## API

Start an aggregate query with `.aggregate(where_input)`, chain operations, then call `.exec()`:

```rust
client
    .model()
    .aggregate(WhereInput::default())
    .min(field)
    .max(field)
    .avg(field)   // numeric fields only
    .sum(field)   // numeric fields only
    .exec()
    .await?
```

At least one operation must be specified or `.exec()` returns an error.

## UserAggregateField

Each model generates an enum listing the fields that support aggregation:

```rust
pub enum UserAggregateField {
    CreatedAt,
    UpdatedAt,
}
```

### Which operations are allowed?

| Field type | `min` | `max` | `avg` | `sum` |
|---|---|---|---|---|
| `Int`, `BigInt`, `Float` | Yes | Yes | Yes | Yes |
| `DateTime` | Yes | Yes | No | No |
| `String`, `Boolean`, `Enum` | No | No | No | No |

Calling `avg()` or `sum()` on a non-numeric field will panic at runtime with a clear message.

## UserAggregateResult

The return type contains an `Option` field for each possible operation+field combination:

```rust
pub struct UserAggregateResult {
    pub min_created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub max_created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub min_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub max_updated_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

Fields that were not requested in the query will be `None` (via `#[sqlx(default)]`).

For models with numeric fields, you would also see fields like `avg_age`, `sum_score`, etc.

## Aggregate with Filters

Pass a `WhereInput` to compute aggregates over a subset of records:

```rust
use generated::user::filter::UserWhereInput;
use ferriorm_runtime::filter::EnumFilter;
use generated::Role;

let result = client
    .user()
    .aggregate(UserWhereInput {
        role: Some(EnumFilter {
            equals: Some(Role::Admin),
            ..Default::default()
        }),
        ..Default::default()
    })
    .min(UserAggregateField::CreatedAt)
    .max(UserAggregateField::CreatedAt)
    .exec()
    .await?;

println!("First admin created: {:?}", result.min_created_at);
```

## Numeric Aggregate Example

For a model with numeric fields (e.g., `Order` with a `total` field of type `Float`):

```rust
let stats = client
    .order()
    .aggregate(OrderWhereInput {
        status: Some(EnumFilter {
            equals: Some(OrderStatus::Completed),
            ..Default::default()
        }),
        ..Default::default()
    })
    .sum(OrderAggregateField::Total)
    .avg(OrderAggregateField::Total)
    .min(OrderAggregateField::Total)
    .max(OrderAggregateField::Total)
    .exec()
    .await?;

println!("Revenue: {:?}", stats.sum_total);
println!("Average order: {:?}", stats.avg_total);
```

## Group By

`group_by` buckets rows by one or more columns and computes aggregates per
bucket -- the SQL equivalent of `SELECT keys, AGG(...) FROM t GROUP BY keys`.
Each model with at least one groupable scalar field gets a `group_by()` method
on its actions, plus a generated `<Model>GroupByField` enum and
`<Model>GroupByResult` row struct.

### Basic Usage

```rust
use generated::user::UserGroupByField;

let buckets = client
    .user()
    .group_by(vec![UserGroupByField::Role])
    .count()
    .exec()
    .await?;

for b in &buckets {
    println!("role={:?} users={:?}", b.role, b.count);
}
```

`group_by` returns `Vec<<Model>GroupByResult>` where each row exposes:

- An `Option<T>` field for **every** groupable column on the model (only the
  columns passed to `group_by(vec![...])` are populated; the rest are `None`).
- `count: Option<i64>`, populated when `.count()` is called.
- `avg_<col>` / `sum_<col>` (`Option<f64>`) and `min_<col>` / `max_<col>` for
  every numeric / `DateTime` column on the model -- populated only for the
  ones requested via `.avg(...)`, `.sum(...)`, `.min(...)`, `.max(...)`.

### Combining `group_by` with Aggregates

The aggregate methods on `GroupByQuery` accept the same
`<Model>AggregateField` variants used by `aggregate()`:

```rust
use generated::order::{OrderGroupByField, OrderAggregateField};

let buckets = client
    .order()
    .group_by(vec![OrderGroupByField::CustomerId])
    .count()
    .sum(OrderAggregateField::Total)
    .avg(OrderAggregateField::Total)
    .exec()
    .await?;

for b in &buckets {
    println!(
        "customer={:?} orders={:?} revenue={:?} avg={:?}",
        b.customer_id, b.count, b.sum_total, b.avg_total,
    );
}
```

### Multiple Group Keys

Pass multiple variants to bucket on the cross-product of columns:

```rust
let buckets = client
    .order()
    .group_by(vec![
        OrderGroupByField::CustomerId,
        OrderGroupByField::Status,
    ])
    .count()
    .exec()
    .await?;
```

Each `b.customer_id` and `b.status` is populated; the other key columns on
the model stay `None`.

### Filtering Source Rows with `WHERE`

Apply a `WhereInput` before grouping using `.r#where(...)`. This is the SQL
`WHERE` clause -- it filters rows **before** the grouping step:

```rust
use generated::order::filter::OrderWhereInput;
use ferriorm_runtime::filter::DateTimeFilter;

let recent = client
    .order()
    .group_by(vec![OrderGroupByField::CustomerId])
    .r#where(OrderWhereInput {
        created_at: Some(DateTimeFilter {
            gte: Some(thirty_days_ago),
            ..Default::default()
        }),
        ..Default::default()
    })
    .count()
    .exec()
    .await?;
```

### Filtering Buckets with `HAVING`

`having()` filters the **post-aggregation** result -- the SQL `HAVING` clause.
The generated `<Model>HavingInput` mirrors `WhereInput` but its fields target
aggregate expressions:

- `count: Option<BigIntFilter>` -- filters on `COUNT(*)`.
- For each numeric column `col`:
  - `avg_<col>: Option<FloatFilter>`
  - `sum_<col>: Option<FloatFilter>`
  - `min_<col>: Option<<col_filter>>`
  - `max_<col>: Option<<col_filter>>`
- For each `DateTime` column `col`:
  - `min_<col>: Option<DateTimeFilter>`
  - `max_<col>: Option<DateTimeFilter>`
- Compose with `and: Option<Vec<Self>>`, `or: Option<Vec<Self>>`,
  `not: Option<Box<Self>>`.

```rust
use generated::order::OrderHavingInput;
use ferriorm_runtime::filter::{BigIntFilter, FloatFilter};

let big_spenders = client
    .order()
    .group_by(vec![OrderGroupByField::CustomerId])
    .count()
    .sum(OrderAggregateField::Total)
    .having(OrderHavingInput {
        count: Some(BigIntFilter {
            gte: Some(5),
            ..Default::default()
        }),
        sum_total: Some(FloatFilter {
            gt: Some(1000.0),
            ..Default::default()
        }),
        ..Default::default()
    })
    .exec()
    .await?;
```

The example above selects customers who placed **5 or more orders** **and**
spent **more than $1000** in total.

Each HAVING field supports the same operators as the underlying scalar filter,
including `r#in` and `not_in`. They follow the same empty-list rules as
[`WHERE` IN](filtering.md#in--not-in-semantics): an empty `r#in` becomes
`HAVING ... AND 1 = 0` (no rows), and an empty `not_in` is dropped.

### Allowed Group Keys

A field is groupable if it is a scalar of type `String`, `Int`, `BigInt`,
`Float`, `Boolean`, `DateTime`, or any user-defined `enum`. `Json`, `Bytes`,
`Decimal`, and relations are excluded -- they are not hashable / orderable
in SQL.

### Limitations

`group_by()` is intentionally narrow in v1 and does not currently support:

- `ORDER BY` on the bucket result -- buckets come back in database-defined
  order. Sort in Rust if you need a deterministic order.
- `skip` / `take` (pagination) at the group level.
- `COUNT(DISTINCT col)`.
- Grouping by a relation column (use the foreign-key scalar instead, e.g.
  `OrderGroupByField::CustomerId`).
