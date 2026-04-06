/// Supported database providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseProvider {
    PostgreSQL,
    SQLite,
    MySQL,
}

impl DatabaseProvider {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "postgresql" | "postgres" => Some(Self::PostgreSQL),
            "sqlite" => Some(Self::SQLite),
            "mysql" => Some(Self::MySQL),
            _ => None,
        }
    }

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

impl ScalarType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "String" => Some(Self::String),
            "Int" => Some(Self::Int),
            "BigInt" => Some(Self::BigInt),
            "Float" => Some(Self::Float),
            "Decimal" => Some(Self::Decimal),
            "Boolean" | "Bool" => Some(Self::Boolean),
            "DateTime" => Some(Self::DateTime),
            "Json" => Some(Self::Json),
            "Bytes" => Some(Self::Bytes),
            _ => None,
        }
    }

    /// The Rust type this scalar maps to.
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

    /// The PostgreSQL column type.
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

    /// The SQLite column type.
    pub fn sqlite_type(&self) -> &'static str {
        match self {
            Self::String => "TEXT",
            Self::Int => "INTEGER",
            Self::BigInt => "INTEGER",
            Self::Float => "REAL",
            Self::Decimal => "TEXT",
            Self::Boolean => "INTEGER",
            Self::DateTime => "TEXT",
            Self::Json => "TEXT",
            Self::Bytes => "BLOB",
        }
    }
}
