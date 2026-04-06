/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// Trait that all generated OrderBy types implement.
pub trait OrderByClause {
    /// Append the ORDER BY column and direction to the SQL builder.
    fn apply_to(&self, builder: &mut crate::query::SqlBuilder);
}
