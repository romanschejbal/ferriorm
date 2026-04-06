use ormx_core::schema::Schema;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Generate the `client.rs` module with the OrmxClient struct.
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
        use ormx_runtime::prelude::*;

        pub struct OrmxClient {
            inner: DatabaseClient,
        }

        impl OrmxClient {
            /// Connect to the database using the provided URL.
            pub async fn connect(url: &str) -> Result<Self, OrmxError> {
                let inner = DatabaseClient::connect(url).await?;
                Ok(Self { inner })
            }

            /// Get a reference to the underlying database client.
            pub fn client(&self) -> &DatabaseClient {
                &self.inner
            }

            #(#model_accessors)*

            /// Close the database connection.
            pub async fn disconnect(self) {
                self.inner.disconnect().await;
            }
        }
    }
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_lowercase().next().unwrap());
    }
    result
}
