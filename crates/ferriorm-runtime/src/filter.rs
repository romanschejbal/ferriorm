//! Type-safe filter types for query WHERE clauses.
//!
//! Each scalar type has a corresponding filter struct (e.g., [`StringFilter`],
//! [`IntFilter`], [`DateTimeFilter`]) with fields like `equals`, `not`,
//! `contains`, `gt`, `lt`, `in`, etc. Enums use the generic [`EnumFilter<E>`].
//!
//! Generated `WhereInput` structs compose these filters and implement the
//! `WhereClause` trait to append SQL conditions to a `SqlBuilder`.

/// Filter operations for String fields.
#[derive(Debug, Clone, Default)]
pub struct StringFilter {
    pub equals: Option<String>,
    pub not: Option<String>,
    pub contains: Option<String>,
    pub starts_with: Option<String>,
    pub ends_with: Option<String>,
    pub r#in: Option<Vec<String>>,
    pub not_in: Option<Vec<String>>,
    pub mode: Option<QueryMode>,
}

/// Filter operations for nullable String fields.
#[derive(Debug, Clone, Default)]
pub struct NullableStringFilter {
    /// `Some(None)` means IS NULL, `Some(Some(v))` means equals v.
    pub equals: Option<Option<String>>,
    pub not: Option<Option<String>>,
    pub contains: Option<String>,
    pub starts_with: Option<String>,
    pub ends_with: Option<String>,
    pub r#in: Option<Vec<String>>,
    pub not_in: Option<Vec<String>>,
    pub mode: Option<QueryMode>,
}

/// Filter operations for i32 fields.
#[derive(Debug, Clone, Default)]
pub struct IntFilter {
    pub equals: Option<i32>,
    pub not: Option<i32>,
    pub gt: Option<i32>,
    pub gte: Option<i32>,
    pub lt: Option<i32>,
    pub lte: Option<i32>,
    pub r#in: Option<Vec<i32>>,
    pub not_in: Option<Vec<i32>>,
}

/// Filter operations for nullable i32 fields.
#[derive(Debug, Clone, Default)]
pub struct NullableIntFilter {
    /// `Some(None)` means IS NULL, `Some(Some(v))` means equals v.
    pub equals: Option<Option<i32>>,
    pub not: Option<Option<i32>>,
    pub gt: Option<i32>,
    pub gte: Option<i32>,
    pub lt: Option<i32>,
    pub lte: Option<i32>,
    pub r#in: Option<Vec<i32>>,
    pub not_in: Option<Vec<i32>>,
}

/// Filter operations for i64 fields.
#[derive(Debug, Clone, Default)]
pub struct BigIntFilter {
    pub equals: Option<i64>,
    pub not: Option<i64>,
    pub gt: Option<i64>,
    pub gte: Option<i64>,
    pub lt: Option<i64>,
    pub lte: Option<i64>,
    pub r#in: Option<Vec<i64>>,
    pub not_in: Option<Vec<i64>>,
}

/// Filter operations for nullable i64 fields.
#[derive(Debug, Clone, Default)]
pub struct NullableBigIntFilter {
    /// `Some(None)` means IS NULL, `Some(Some(v))` means equals v.
    pub equals: Option<Option<i64>>,
    pub not: Option<Option<i64>>,
    pub gt: Option<i64>,
    pub gte: Option<i64>,
    pub lt: Option<i64>,
    pub lte: Option<i64>,
    pub r#in: Option<Vec<i64>>,
    pub not_in: Option<Vec<i64>>,
}

/// Filter operations for f64 fields.
#[derive(Debug, Clone, Default)]
pub struct FloatFilter {
    pub equals: Option<f64>,
    pub not: Option<f64>,
    pub gt: Option<f64>,
    pub gte: Option<f64>,
    pub lt: Option<f64>,
    pub lte: Option<f64>,
}

/// Filter operations for nullable f64 fields.
#[derive(Debug, Clone, Default)]
pub struct NullableFloatFilter {
    /// `Some(None)` means IS NULL, `Some(Some(v))` means equals v.
    pub equals: Option<Option<f64>>,
    pub not: Option<Option<f64>>,
    pub gt: Option<f64>,
    pub gte: Option<f64>,
    pub lt: Option<f64>,
    pub lte: Option<f64>,
}

/// Filter operations for bool fields.
#[derive(Debug, Clone, Default)]
pub struct BoolFilter {
    pub equals: Option<bool>,
    pub not: Option<bool>,
}

/// Filter operations for nullable bool fields.
#[derive(Debug, Clone, Default)]
pub struct NullableBoolFilter {
    /// `Some(None)` means IS NULL, `Some(Some(v))` means equals v.
    pub equals: Option<Option<bool>>,
    pub not: Option<Option<bool>>,
}

/// Filter operations for `DateTime` fields.
#[derive(Debug, Clone, Default)]
pub struct DateTimeFilter {
    pub equals: Option<chrono::DateTime<chrono::Utc>>,
    pub not: Option<chrono::DateTime<chrono::Utc>>,
    pub gt: Option<chrono::DateTime<chrono::Utc>>,
    pub gte: Option<chrono::DateTime<chrono::Utc>>,
    pub lt: Option<chrono::DateTime<chrono::Utc>>,
    pub lte: Option<chrono::DateTime<chrono::Utc>>,
    pub r#in: Option<Vec<chrono::DateTime<chrono::Utc>>>,
}

/// Filter operations for nullable `DateTime` fields.
#[derive(Debug, Clone, Default)]
pub struct NullableDateTimeFilter {
    /// `Some(None)` means IS NULL, `Some(Some(v))` means equals v.
    pub equals: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub not: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub gt: Option<chrono::DateTime<chrono::Utc>>,
    pub gte: Option<chrono::DateTime<chrono::Utc>>,
    pub lt: Option<chrono::DateTime<chrono::Utc>>,
    pub lte: Option<chrono::DateTime<chrono::Utc>>,
    pub r#in: Option<Vec<chrono::DateTime<chrono::Utc>>>,
}

/// Filter operations for enum fields (generic over the enum type).
#[derive(Debug, Clone)]
pub struct EnumFilter<E: Clone> {
    pub equals: Option<E>,
    pub not: Option<E>,
    pub r#in: Option<Vec<E>>,
    pub not_in: Option<Vec<E>>,
}

impl<E: Clone> Default for EnumFilter<E> {
    fn default() -> Self {
        Self {
            equals: None,
            not: None,
            r#in: None,
            not_in: None,
        }
    }
}

/// Query mode for string operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    Default,
    Insensitive,
}
