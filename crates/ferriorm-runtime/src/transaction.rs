//! Database transaction support.
//!
//! Provides [`run_transaction`] for executing a closure within a database
//! transaction, and [`TransactionClient`] as a wrapper around sqlx transaction
//! handles. The transaction is automatically committed on success or rolled
//! back on error.

use crate::client::DatabaseClient;
use crate::error::FerriormError;

/// Execute a closure within a database transaction.
///
/// The closure receives a [`TransactionClient`] and must return it alongside the
/// result on success so the transaction can be committed. If the closure returns
/// `Err`, the transaction is rolled back automatically (sqlx rolls back on drop).
///
/// # Example
///
/// ```ignore
/// let result = run_transaction(&client, |tx| async move {
///     // ... use tx for queries ...
///     Ok((value, tx))
/// }).await?;
/// ```
///
/// # Errors
///
/// Returns a [`FerriormError`] if the transaction fails or the closure returns an error.
pub async fn run_transaction<F, Fut, T>(client: &DatabaseClient, f: F) -> Result<T, FerriormError>
where
    F: FnOnce(TransactionClient) -> Fut,
    Fut: std::future::Future<Output = Result<(T, TransactionClient), FerriormError>>,
{
    match client {
        #[cfg(feature = "postgres")]
        DatabaseClient::Postgres(pool) => {
            let tx = pool.begin().await?;
            let tx_client = TransactionClient::Postgres(tx);
            match f(tx_client).await {
                Ok((result, tx_client)) => {
                    tx_client.commit().await?;
                    Ok(result)
                }
                Err(e) => {
                    // Transaction is dropped here, which triggers auto-rollback.
                    Err(e)
                }
            }
        }
        #[cfg(feature = "sqlite")]
        DatabaseClient::Sqlite(pool) => {
            let tx = pool.begin().await?;
            let tx_client = TransactionClient::Sqlite(tx);
            match f(tx_client).await {
                Ok((result, tx_client)) => {
                    tx_client.commit().await?;
                    Ok(result)
                }
                Err(e) => {
                    // Transaction is dropped here, which triggers auto-rollback.
                    Err(e)
                }
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
    ///
    /// # Errors
    ///
    /// Returns a [`FerriormError`] if the commit fails.
    pub async fn commit(self) -> Result<(), FerriormError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(tx) => tx.commit().await.map_err(FerriormError::from),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(tx) => tx.commit().await.map_err(FerriormError::from),
        }
    }

    /// Rollback the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`FerriormError`] if the rollback fails.
    pub async fn rollback(self) -> Result<(), FerriormError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(tx) => tx.rollback().await.map_err(FerriormError::from),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(tx) => tx.rollback().await.map_err(FerriormError::from),
        }
    }
}
