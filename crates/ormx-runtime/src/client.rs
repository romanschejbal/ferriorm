use crate::error::OrmxError;

/// The database client, wrapping an sqlx connection pool.
///
/// Supports PostgreSQL and SQLite via feature flags.
/// When both features are enabled, it can connect to either.
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
    pub async fn connect_postgres(url: &str) -> Result<Self, OrmxError> {
        let pool = sqlx::PgPool::connect(url).await?;
        Ok(Self::Postgres(pool))
    }

    /// Connect to a SQLite database.
    #[cfg(feature = "sqlite")]
    pub async fn connect_sqlite(url: &str) -> Result<Self, OrmxError> {
        let pool = sqlx::SqlitePool::connect(url).await?;
        Ok(Self::Sqlite(pool))
    }

    /// Connect by auto-detecting the database type from the URL.
    pub async fn connect(url: &str) -> Result<Self, OrmxError> {
        #[cfg(feature = "sqlite")]
        if url.starts_with("sqlite:") || url.starts_with("file:") || url.ends_with(".db") {
            return Self::connect_sqlite(url).await;
        }

        #[cfg(feature = "postgres")]
        {
            return Self::connect_postgres(url).await;
        }

        #[allow(unreachable_code)]
        Err(OrmxError::Connection(
            "No database backend enabled. Enable 'postgres' or 'sqlite' feature.".into(),
        ))
    }

    /// Returns true if this client is connected to PostgreSQL.
    #[allow(unreachable_patterns)]
    pub fn is_postgres(&self) -> bool {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => true,
            _ => false,
        }
    }

    /// Returns true if this client is connected to SQLite.
    #[allow(unreachable_patterns)]
    pub fn is_sqlite(&self) -> bool {
        match self {
            #[cfg(feature = "sqlite")]
            Self::Sqlite(_) => true,
            _ => false,
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
}
