use crate::client::DatabaseClient;
use crate::error::OrmxError;

/// Execute a closure within a database transaction.
///
/// If the closure returns `Ok`, the transaction is committed.
/// If it returns `Err` or panics, the transaction is rolled back.
pub async fn run_transaction<F, Fut, T>(client: &DatabaseClient, f: F) -> Result<T, OrmxError>
where
    F: FnOnce(TransactionClient) -> Fut,
    Fut: std::future::Future<Output = Result<T, OrmxError>>,
{
    match client {
        #[cfg(feature = "postgres")]
        DatabaseClient::Postgres(pool) => {
            let tx = pool.begin().await?;
            let tx_client = TransactionClient::Postgres(tx);
            match f(tx_client).await {
                Ok(result) => Ok(result),
                Err(e) => Err(e),
            }
        }
        #[cfg(feature = "sqlite")]
        DatabaseClient::Sqlite(pool) => {
            let tx = pool.begin().await?;
            let tx_client = TransactionClient::Sqlite(tx);
            match f(tx_client).await {
                Ok(result) => Ok(result),
                Err(e) => Err(e),
            }
        }
    }
}

/// A client wrapper for use within transactions.
pub enum TransactionClient {
    #[cfg(feature = "postgres")]
    Postgres(sqlx::Transaction<'static, sqlx::Postgres>),
    #[cfg(feature = "sqlite")]
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
}

impl TransactionClient {
    /// Commit the transaction.
    pub async fn commit(self) -> Result<(), OrmxError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(tx) => tx.commit().await.map_err(OrmxError::from),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(tx) => tx.commit().await.map_err(OrmxError::from),
        }
    }

    /// Rollback the transaction.
    pub async fn rollback(self) -> Result<(), OrmxError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(tx) => tx.rollback().await.map_err(OrmxError::from),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(tx) => tx.rollback().await.map_err(OrmxError::from),
        }
    }
}
