//! Ordering support for query results.
//!
//! Defines [`SortOrder`] (ascending / descending) and the `OrderByClause`
//! trait that generated per-model `OrderByInput` enums implement. This allows
//! query builders to chain `.order_by(...)` calls in a type-safe way.

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    /// Returns the SQL keyword for this sort direction.
    #[must_use]
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}
