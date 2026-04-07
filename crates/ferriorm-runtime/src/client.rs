//! Database connection pool wrapper.
//!
//! [`DatabaseClient`] is an enum that wraps either a PostgreSQL or SQLite
//! connection pool (via sqlx). It provides auto-detection from the connection
//! URL and exposes typed `fetch_all`, `fetch_optional`, `fetch_one`, and
//! `execute` helpers used by the generated query builders.
//!
//! [`PoolConfig`] allows fine-grained control over the underlying connection
//! pool (max/min connections, timeouts, etc.).

use std::time::Duration;

use crate::error::FerriormError;

/// Configuration options for the database connection pool.
///
/// All fields are optional; when `None`, the sqlx defaults are used.
///
/// # Example
///
/// ```rust
/// use ferriorm_runtime::client::PoolConfig;
/// use std::time::Duration;
///
/// let config = PoolConfig {
///     max_connections: Some(20),
///     idle_timeout: Some(Duration::from_secs(300)),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool.
    pub max_connections: Option<u32>,
    /// Minimum number of connections to keep open at all times.
    pub min_connections: Option<u32>,
    /// Maximum time a connection can sit idle before being closed.
    pub idle_timeout: Option<Duration>,
    /// Maximum lifetime of a connection before it is closed and replaced.
    pub max_lifetime: Option<Duration>,
    /// Maximum time to wait when acquiring a connection from the pool.
    pub acquire_timeout: Option<Duration>,
}

/// The database client, wrapping an sqlx connection pool.
///
/// Supports PostgreSQL and SQLite via feature flags.
#[derive(Debug, Clone)]
pub enum DatabaseClient {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::SqlitePool),
}

impl DatabaseClient {
    /// Connect to a PostgreSQL database.
    #[cfg(feature = "postgres")]
    pub async fn connect_postgres(url: &str) -> Result<Self, FerriormError> {
        let pool = sqlx::PgPool::connect(url).await?;
        Ok(Self::Postgres(pool))
    }

    /// Connect to a PostgreSQL database with custom pool configuration.
    #[cfg(feature = "postgres")]
    pub async fn connect_postgres_with_config(
        url: &str,
        config: &PoolConfig,
    ) -> Result<Self, FerriormError> {
        let mut opts = sqlx::postgres::PgPoolOptions::new();
        if let Some(max) = config.max_connections {
            opts = opts.max_connections(max);
        }
        if let Some(min) = config.min_connections {
            opts = opts.min_connections(min);
        }
        if let Some(timeout) = config.idle_timeout {
            opts = opts.idle_timeout(timeout);
        }
        if let Some(lifetime) = config.max_lifetime {
            opts = opts.max_lifetime(lifetime);
        }
        if let Some(timeout) = config.acquire_timeout {
            opts = opts.acquire_timeout(timeout);
        }
        let pool = opts.connect(url).await?;
        Ok(Self::Postgres(pool))
    }

    /// Connect to a SQLite database.
    #[cfg(feature = "sqlite")]
    pub async fn connect_sqlite(url: &str) -> Result<Self, FerriormError> {
        let pool = sqlx::SqlitePool::connect(url).await?;
        Ok(Self::Sqlite(pool))
    }

    /// Connect to a SQLite database with custom pool configuration.
    #[cfg(feature = "sqlite")]
    pub async fn connect_sqlite_with_config(
        url: &str,
        config: &PoolConfig,
    ) -> Result<Self, FerriormError> {
        let mut opts = sqlx::sqlite::SqlitePoolOptions::new();
        if let Some(max) = config.max_connections {
            opts = opts.max_connections(max);
        }
        if let Some(min) = config.min_connections {
            opts = opts.min_connections(min);
        }
        if let Some(timeout) = config.idle_timeout {
            opts = opts.idle_timeout(timeout);
        }
        if let Some(lifetime) = config.max_lifetime {
            opts = opts.max_lifetime(lifetime);
        }
        if let Some(timeout) = config.acquire_timeout {
            opts = opts.acquire_timeout(timeout);
        }
        let pool = opts.connect(url).await?;
        Ok(Self::Sqlite(pool))
    }

    /// Connect by auto-detecting the database type from the URL.
    pub async fn connect(url: &str) -> Result<Self, FerriormError> {
        #[cfg(feature = "sqlite")]
        if url.starts_with("sqlite:") || url.starts_with("file:") || url.ends_with(".db") {
            return Self::connect_sqlite(url).await;
        }

        #[cfg(feature = "postgres")]
        {
            return Self::connect_postgres(url).await;
        }

        #[allow(unreachable_code)]
        Err(FerriormError::Connection(
            "No database backend enabled. Enable 'postgres' or 'sqlite' feature.".into(),
        ))
    }

    /// Connect by auto-detecting the database type from the URL, using custom
    /// pool configuration.
    pub async fn connect_with_config(
        url: &str,
        config: &PoolConfig,
    ) -> Result<Self, FerriormError> {
        #[cfg(feature = "sqlite")]
        if url.starts_with("sqlite:") || url.starts_with("file:") || url.ends_with(".db") {
            return Self::connect_sqlite_with_config(url, config).await;
        }

        #[cfg(feature = "postgres")]
        {
            return Self::connect_postgres_with_config(url, config).await;
        }

        #[allow(unreachable_code)]
        Err(FerriormError::Connection(
            "No database backend enabled. Enable 'postgres' or 'sqlite' feature.".into(),
        ))
    }

    /// Get a reference to the underlying PostgreSQL pool for raw queries.
    ///
    /// Returns an error if this client is not connected to PostgreSQL.
    #[cfg(feature = "postgres")]
    pub fn pg_pool(&self) -> Result<&sqlx::PgPool, FerriormError> {
        match self {
            Self::Postgres(pool) => Ok(pool),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Connection(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    /// Get a reference to the underlying SQLite pool for raw queries.
    ///
    /// Returns an error if this client is not connected to SQLite.
    #[cfg(feature = "sqlite")]
    pub fn sqlite_pool(&self) -> Result<&sqlx::SqlitePool, FerriormError> {
        match self {
            Self::Sqlite(pool) => Ok(pool),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Connection(
                "Expected SQLite connection".into(),
            )),
        }
    }

    /// Close the connection pool.
    pub async fn disconnect(self) {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(pool) => pool.close().await,
            #[cfg(feature = "sqlite")]
            Self::Sqlite(pool) => pool.close().await,
        }
    }

    // ── Raw SQL helpers (PostgreSQL) ────────────────────────────────────

    /// Execute raw SQL and return all rows mapped to `T` (PostgreSQL).
    #[cfg(feature = "postgres")]
    pub async fn raw_fetch_all_pg<T>(&self, sql: &str) -> Result<Vec<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        match self {
            Self::Postgres(pool) => Ok(sqlx::query_as::<_, T>(sql).fetch_all(pool).await?),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    /// Execute raw SQL and return exactly one row mapped to `T` (PostgreSQL).
    #[cfg(feature = "postgres")]
    pub async fn raw_fetch_one_pg<T>(&self, sql: &str) -> Result<T, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        match self {
            Self::Postgres(pool) => Ok(sqlx::query_as::<_, T>(sql).fetch_one(pool).await?),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    /// Execute raw SQL and return an optional row mapped to `T` (PostgreSQL).
    #[cfg(feature = "postgres")]
    pub async fn raw_fetch_optional_pg<T>(&self, sql: &str) -> Result<Option<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        match self {
            Self::Postgres(pool) => Ok(sqlx::query_as::<_, T>(sql).fetch_optional(pool).await?),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    /// Execute raw SQL without returning rows (PostgreSQL). Returns the
    /// number of rows affected.
    #[cfg(feature = "postgres")]
    pub async fn raw_execute_pg(&self, sql: &str) -> Result<u64, FerriormError> {
        match self {
            Self::Postgres(pool) => Ok(sqlx::query(sql).execute(pool).await?.rows_affected()),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    // ── Raw SQL helpers (SQLite) ────────────────────────────────────────

    /// Execute raw SQL and return all rows mapped to `T` (SQLite).
    #[cfg(feature = "sqlite")]
    pub async fn raw_fetch_all_sqlite<T>(&self, sql: &str) -> Result<Vec<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
    {
        match self {
            Self::Sqlite(pool) => Ok(sqlx::query_as::<_, T>(sql).fetch_all(pool).await?),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    /// Execute raw SQL and return exactly one row mapped to `T` (SQLite).
    #[cfg(feature = "sqlite")]
    pub async fn raw_fetch_one_sqlite<T>(&self, sql: &str) -> Result<T, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
    {
        match self {
            Self::Sqlite(pool) => Ok(sqlx::query_as::<_, T>(sql).fetch_one(pool).await?),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    /// Execute raw SQL and return an optional row mapped to `T` (SQLite).
    #[cfg(feature = "sqlite")]
    pub async fn raw_fetch_optional_sqlite<T>(&self, sql: &str) -> Result<Option<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
    {
        match self {
            Self::Sqlite(pool) => Ok(sqlx::query_as::<_, T>(sql).fetch_optional(pool).await?),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    /// Execute raw SQL without returning rows (SQLite). Returns the number
    /// of rows affected.
    #[cfg(feature = "sqlite")]
    pub async fn raw_execute_sqlite(&self, sql: &str) -> Result<u64, FerriormError> {
        match self {
            Self::Sqlite(pool) => Ok(sqlx::query(sql).execute(pool).await?.rows_affected()),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    /// Execute a query builder against the appropriate pool, returning all rows.
    #[cfg(feature = "postgres")]
    pub async fn fetch_all_pg<'q, T>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Postgres>,
    ) -> Result<Vec<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        match self {
            Self::Postgres(pool) => Ok(qb.build_query_as::<T>().fetch_all(pool).await?),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub async fn fetch_optional_pg<'q, T>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Postgres>,
    ) -> Result<Option<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        match self {
            Self::Postgres(pool) => Ok(qb.build_query_as::<T>().fetch_optional(pool).await?),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub async fn fetch_one_pg<'q, T>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Postgres>,
    ) -> Result<T, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        match self {
            Self::Postgres(pool) => Ok(qb.build_query_as::<T>().fetch_one(pool).await?),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub async fn execute_pg<'q>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Postgres>,
    ) -> Result<u64, FerriormError> {
        match self {
            Self::Postgres(pool) => Ok(qb.build().execute(pool).await?.rows_affected()),
            #[cfg(feature = "sqlite")]
            _ => Err(FerriormError::Query(
                "Expected PostgreSQL connection".into(),
            )),
        }
    }

    // SQLite variants
    #[cfg(feature = "sqlite")]
    pub async fn fetch_all_sqlite<'q, T>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Sqlite>,
    ) -> Result<Vec<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
    {
        match self {
            Self::Sqlite(pool) => Ok(qb.build_query_as::<T>().fetch_all(pool).await?),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    #[cfg(feature = "sqlite")]
    pub async fn fetch_optional_sqlite<'q, T>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Sqlite>,
    ) -> Result<Option<T>, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
    {
        match self {
            Self::Sqlite(pool) => Ok(qb.build_query_as::<T>().fetch_optional(pool).await?),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    #[cfg(feature = "sqlite")]
    pub async fn fetch_one_sqlite<'q, T>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Sqlite>,
    ) -> Result<T, FerriormError>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
    {
        match self {
            Self::Sqlite(pool) => Ok(qb.build_query_as::<T>().fetch_one(pool).await?),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }

    #[cfg(feature = "sqlite")]
    pub async fn execute_sqlite<'q>(
        &self,
        mut qb: sqlx::QueryBuilder<'q, sqlx::Sqlite>,
    ) -> Result<u64, FerriormError> {
        match self {
            Self::Sqlite(pool) => Ok(qb.build().execute(pool).await?.rows_affected()),
            #[cfg(feature = "postgres")]
            _ => Err(FerriormError::Query("Expected SQLite connection".into())),
        }
    }
}
