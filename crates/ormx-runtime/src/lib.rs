pub mod client;
pub mod error;
pub mod filter;
pub mod order;
pub mod query;
pub mod transaction;

pub mod prelude {
    pub use crate::client::DatabaseClient;
    pub use crate::error::OrmxError;
    pub use crate::filter::*;
    pub use crate::order::{OrderByClause, SortOrder};
    pub use crate::query::SqlBuilder;
    pub use crate::SetValue;
}

/// Wrapper for update operations: distinguishes "not set" from "set to value".
#[derive(Debug, Clone)]
pub enum SetValue<T> {
    Set(T),
}
