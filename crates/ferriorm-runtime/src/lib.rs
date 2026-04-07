//! Runtime library shipped with the user's application.
//!
//! This crate provides everything the generated ferriorm client needs at runtime:
//!
//! - [`client::DatabaseClient`] -- connection pool wrapper (PostgreSQL + SQLite)
//! - [`filter`] -- type-safe filter structs (`StringFilter`, `IntFilter`, ...)
//! - [`query::SqlBuilder`] -- parameterized SQL builder supporting `$1` and `?` styles
//! - [`order`] -- `SortOrder` and `OrderByClause` trait
//! - [`transaction`] -- transaction execution helpers
//! - [`error::FerriormError`] -- unified error type
//!
//! A [`prelude`] module re-exports the most commonly used items.
//!
//! # Related crates
//!
//! - `ferriorm_core` -- domain types (this crate does not depend on `ferriorm-core`
//!   at runtime; the generated code bridges the two).
//! - `ferriorm_codegen` -- generates code that depends on this crate.

pub mod client;
pub mod error;
pub mod filter;
pub mod order;
pub mod query;
pub mod transaction;

pub mod prelude {
    pub use crate::SetValue;
    pub use crate::client::DatabaseClient;
    pub use crate::client::PoolConfig;
    pub use crate::error::FerriormError;
    pub use crate::filter::*;
    pub use crate::order::SortOrder;
    pub use crate::query::SqlBuilder;
    pub use chrono;
    pub use sqlx;
    pub use uuid;
}

/// Wrapper for update operations: distinguishes "not set" from "set to value".
#[derive(Debug, Clone)]
pub enum SetValue<T> {
    Set(T),
}
