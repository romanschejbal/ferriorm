//! Generates the `enums.rs` module from schema enum definitions.
//!
//! Each schema enum becomes a Rust enum with `sqlx::Type`, `Serialize`,
//! `Deserialize`, and `Display` derives, ready for use in queries and
//! serialization.

use ferriorm_core::schema::Enum;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Generate the `enums.rs` module containing all enum definitions.
#[must_use]
pub fn generate_enums_module(enums: &[Enum]) -> TokenStream {
    if enums.is_empty() {
        return quote! {};
    }

    let enum_defs: Vec<TokenStream> = enums.iter().map(generate_enum).collect();

    quote! {
        #![allow(unused_imports, dead_code, unused_variables, clippy::all, clippy::pedantic, clippy::nursery)]

        use serde::{Deserialize, Serialize};
        use ferriorm_runtime::prelude::sqlx;

        #(#enum_defs)*
    }
}

fn generate_enum(e: &Enum) -> TokenStream {
    let name = format_ident!("{}", e.name);
    let db_name = &e.db_name;
    let variants: Vec<_> = e.variants.iter().map(|v| format_ident!("{}", v)).collect();

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
        #[sqlx(type_name = #db_name, rename_all = "snake_case")]
        pub enum #name {
            #(#variants),*
        }

        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #(Self::#variants => write!(f, stringify!(#variants))),*
                }
            }
        }
    }
}
