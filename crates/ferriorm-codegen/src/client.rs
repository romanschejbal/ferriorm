//! Generates the `client.rs` module containing the `FerriormClient` struct.
//!
//! `FerriormClient` is the user-facing entry point. It wraps a
//! `ferriorm_runtime::client::DatabaseClient` and exposes a method per model
//! (e.g., `.user()`, `.post()`) that returns the model's `Actions` struct
//! for performing CRUD operations.

use ferriorm_core::schema::Schema;
use ferriorm_core::utils::to_snake_case;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Generate the `client.rs` module with the FerriormClient struct.
pub fn generate_client_module(schema: &Schema) -> TokenStream {
    let model_accessors: Vec<TokenStream> = schema
        .models
        .iter()
        .map(|m| {
            let method_name = format_ident!("{}", to_snake_case(&m.name));
            let actions_type = format_ident!("{}Actions", m.name);
            let module_name = format_ident!("{}", to_snake_case(&m.name));

            quote! {
                pub fn #method_name(&self) -> super::#module_name::#actions_type<'_> {
                    super::#module_name::#actions_type::new(&self.inner)
                }
            }
        })
        .collect();

    quote! {
        use ferriorm_runtime::prelude::*;
        use ferriorm_runtime::prelude::sqlx;

        pub struct FerriormClient {
            inner: DatabaseClient,
        }

        impl FerriormClient {
            /// Connect to the database using the provided URL.
            pub async fn connect(url: &str) -> Result<Self, FerriormError> {
                let inner = DatabaseClient::connect(url).await?;
                Ok(Self { inner })
            }

            /// Connect to the database using the provided URL and pool configuration.
            pub async fn connect_with_config(url: &str, config: &PoolConfig) -> Result<Self, FerriormError> {
                let inner = DatabaseClient::connect_with_config(url, config).await?;
                Ok(Self { inner })
            }

            /// Get a reference to the underlying database client.
            pub fn client(&self) -> &DatabaseClient {
                &self.inner
            }

            /// Get a reference to the underlying PostgreSQL pool for raw queries.
            #[cfg(feature = "postgres")]
            pub fn pg_pool(&self) -> Result<&sqlx::PgPool, FerriormError> {
                self.inner.pg_pool()
            }

            /// Get a reference to the underlying SQLite pool for raw queries.
            #[cfg(feature = "sqlite")]
            pub fn sqlite_pool(&self) -> Result<&sqlx::SqlitePool, FerriormError> {
                self.inner.sqlite_pool()
            }

            #(#model_accessors)*

            /// Close the database connection.
            pub async fn disconnect(self) {
                self.inner.disconnect().await;
            }
        }
    }
}
