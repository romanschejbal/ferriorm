//! Mapping from schema types to Rust types for code generation.

use ormx_core::schema::{Field, FieldKind};
use ormx_core::types::ScalarType;
use proc_macro2::TokenStream;
use quote::quote;

/// Nesting depth for module path resolution.
/// `TopLevel` = model module (super::enums::X)
/// `Nested` = inside a submodule like filter/data/order (super::super::enums::X)
#[derive(Debug, Clone, Copy)]
pub enum ModuleDepth {
    TopLevel,
    Nested,
}

/// Returns the token stream for the Rust type of a field.
pub fn rust_type_tokens(field: &Field, depth: ModuleDepth) -> TokenStream {
    let base = match &field.field_type {
        FieldKind::Scalar(scalar) => scalar_to_tokens(scalar),
        FieldKind::Enum(name) => enum_path(name, depth),
        FieldKind::Model(_) => quote! { () },
    };

    if field.is_optional {
        quote! { Option<#base> }
    } else {
        base
    }
}

/// Returns the token stream for an enum reference at the given depth.
pub fn enum_path(name: &str, depth: ModuleDepth) -> TokenStream {
    let ident = quote::format_ident!("{}", name);
    match depth {
        ModuleDepth::TopLevel => quote! { super::enums::#ident },
        ModuleDepth::Nested => quote! { super::super::enums::#ident },
    }
}

/// Returns the token stream for a scalar type.
fn scalar_to_tokens(scalar: &ScalarType) -> TokenStream {
    match scalar {
        ScalarType::String => quote! { String },
        ScalarType::Int => quote! { i32 },
        ScalarType::BigInt => quote! { i64 },
        ScalarType::Float => quote! { f64 },
        ScalarType::Decimal => quote! { String },
        ScalarType::Boolean => quote! { bool },
        ScalarType::DateTime => quote! { chrono::DateTime<chrono::Utc> },
        ScalarType::Json => quote! { serde_json::Value },
        ScalarType::Bytes => quote! { Vec<u8> },
    }
}

/// Returns the filter type name for a field type.
pub fn filter_type_tokens(field: &Field, depth: ModuleDepth) -> Option<TokenStream> {
    match &field.field_type {
        FieldKind::Scalar(scalar) => {
            if field.is_optional {
                match scalar {
                    ScalarType::String => {
                        Some(quote! { ormx_runtime::filter::NullableStringFilter })
                    }
                    _ => scalar_filter_type(scalar),
                }
            } else {
                scalar_filter_type(scalar)
            }
        }
        FieldKind::Enum(name) => {
            let enum_ty = enum_path(name, depth);
            Some(quote! { ormx_runtime::filter::EnumFilter<#enum_ty> })
        }
        FieldKind::Model(_) => None,
    }
}

fn scalar_filter_type(scalar: &ScalarType) -> Option<TokenStream> {
    let tokens = match scalar {
        ScalarType::String => quote! { ormx_runtime::filter::StringFilter },
        ScalarType::Int => quote! { ormx_runtime::filter::IntFilter },
        ScalarType::BigInt => quote! { ormx_runtime::filter::BigIntFilter },
        ScalarType::Float => quote! { ormx_runtime::filter::FloatFilter },
        ScalarType::Boolean => quote! { ormx_runtime::filter::BoolFilter },
        ScalarType::DateTime => quote! { ormx_runtime::filter::DateTimeFilter },
        ScalarType::Json | ScalarType::Bytes | ScalarType::Decimal => return None,
    };
    Some(tokens)
}
