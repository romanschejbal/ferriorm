//! Fundamental type definitions shared across the ferriorm ecosystem.
//!
//! This module defines [`DatabaseProvider`] (`PostgreSQL`, `SQLite`, `MySQL`) and
//! [`ScalarType`] (String, Int, `DateTime`, etc.) along with their mappings to
//! Rust types, `PostgreSQL` column types, and `SQLite` column types.

use std::fmt;
use std::str::FromStr;

/// Supported database providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DatabaseProvider {
    PostgreSQL,
    SQLite,
    MySQL,
}

impl FromStr for DatabaseProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "postgresql" | "postgres" => Ok(Self::PostgreSQL),
            "sqlite" => Ok(Self::SQLite),
            "mysql" => Ok(Self::MySQL),
            _ => Err(format!("unknown database provider: {s}")),
        }
    }
}

impl DatabaseProvider {
    /// Returns the string representation of this database provider.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PostgreSQL => "postgresql",
            Self::SQLite => "sqlite",
            Self::MySQL => "mysql",
        }
    }
}

/// Scalar types supported in the schema language.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ScalarType {
    String,
    Int,
    BigInt,
    Float,
    Decimal,
    Boolean,
    DateTime,
    Json,
    Bytes,
}

impl fmt::Display for ScalarType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl FromStr for ScalarType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "String" => Ok(Self::String),
            "Int" => Ok(Self::Int),
            "BigInt" => Ok(Self::BigInt),
            "Float" => Ok(Self::Float),
            "Decimal" => Ok(Self::Decimal),
            "Boolean" | "Bool" => Ok(Self::Boolean),
            "DateTime" => Ok(Self::DateTime),
            "Json" => Ok(Self::Json),
            "Bytes" => Ok(Self::Bytes),
            _ => Err(format!("unknown scalar type: {s}")),
        }
    }
}

impl ScalarType {
    /// The Rust type this scalar maps to.
    #[must_use]
    pub fn rust_type(&self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Int => "i32",
            Self::BigInt => "i64",
            Self::Float => "f64",
            Self::Decimal => "rust_decimal::Decimal",
            Self::Boolean => "bool",
            Self::DateTime => "chrono::DateTime<chrono::Utc>",
            Self::Json => "serde_json::Value",
            Self::Bytes => "Vec<u8>",
        }
    }

    /// The `PostgreSQL` column type.
    #[must_use]
    pub fn postgres_type(&self) -> &'static str {
        match self {
            Self::String => "TEXT",
            Self::Int => "INTEGER",
            Self::BigInt => "BIGINT",
            Self::Float => "DOUBLE PRECISION",
            Self::Decimal => "DECIMAL",
            Self::Boolean => "BOOLEAN",
            Self::DateTime => "TIMESTAMPTZ",
            Self::Json => "JSONB",
            Self::Bytes => "BYTEA",
        }
    }

    /// The `SQLite` column type.
    #[must_use]
    pub fn sqlite_type(&self) -> &'static str {
        match self {
            Self::String | Self::Decimal | Self::DateTime | Self::Json => "TEXT",
            Self::Int | Self::BigInt | Self::Boolean => "INTEGER",
            Self::Float => "REAL",
            Self::Bytes => "BLOB",
        }
    }
}
