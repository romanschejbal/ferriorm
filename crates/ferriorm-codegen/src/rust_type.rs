//! Mapping from schema types to Rust types for code generation.

use ferriorm_core::schema::{Field, FieldKind};
use ferriorm_core::types::ScalarType;
use proc_macro2::TokenStream;
use quote::quote;

/// Nesting depth for module path resolution.
/// `TopLevel` = model module (`super::enums::X`)
/// `Nested` = inside a submodule like filter/data/order (`super::super::enums::X`)
#[derive(Debug, Clone, Copy)]
pub enum ModuleDepth {
    TopLevel,
    Nested,
}

/// Returns the token stream for the Rust type of a field.
#[must_use]
pub fn rust_type_tokens(field: &Field, depth: ModuleDepth) -> TokenStream {
    let base = match &field.field_type {
        FieldKind::Scalar(scalar) => scalar_to_tokens_with_hint(scalar, field.db_type.as_ref()),
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
#[must_use]
pub fn enum_path(name: &str, depth: ModuleDepth) -> TokenStream {
    let ident = quote::format_ident!("{}", name);
    match depth {
        ModuleDepth::TopLevel => quote! { super::enums::#ident },
        ModuleDepth::Nested => quote! { super::super::enums::#ident },
    }
}

/// Returns the token stream for a scalar type, honouring `@db.*` hints.
fn scalar_to_tokens_with_hint(
    scalar: &ScalarType,
    db_type: Option<&(String, Vec<String>)>,
) -> TokenStream {
    // `@db.BigInt` widens `Int` → `i64`.
    if matches!(scalar, ScalarType::Int) && is_db_bigint(db_type) {
        return quote! { i64 };
    }
    match scalar {
        ScalarType::String | ScalarType::Decimal => quote! { String },
        ScalarType::Int => quote! { i32 },
        ScalarType::BigInt => quote! { i64 },
        ScalarType::Float => quote! { f64 },
        ScalarType::Boolean => quote! { bool },
        ScalarType::DateTime => quote! { chrono::DateTime<chrono::Utc> },
        ScalarType::Json => quote! { serde_json::Value },
        ScalarType::Bytes => quote! { Vec<u8> },
    }
}

fn is_db_bigint(db_type: Option<&(String, Vec<String>)>) -> bool {
    db_type.is_some_and(|(ty, _)| ty == "BigInt")
}

/// Returns the filter type name for a field type.
#[must_use]
pub fn filter_type_tokens(field: &Field, depth: ModuleDepth) -> Option<TokenStream> {
    match &field.field_type {
        FieldKind::Scalar(scalar) => {
            // `@db.BigInt` promotes `Int` to `BigInt` at the filter layer too.
            let effective =
                if matches!(scalar, ScalarType::Int) && is_db_bigint(field.db_type.as_ref()) {
                    ScalarType::BigInt
                } else {
                    scalar.clone()
                };
            if field.is_optional {
                nullable_scalar_filter_type(&effective)
            } else {
                scalar_filter_type(&effective)
            }
        }
        FieldKind::Enum(name) => {
            let enum_ty = enum_path(name, depth);
            Some(quote! { ferriorm_runtime::filter::EnumFilter<#enum_ty> })
        }
        FieldKind::Model(_) => None,
    }
}

fn scalar_filter_type(scalar: &ScalarType) -> Option<TokenStream> {
    let tokens = match scalar {
        ScalarType::String => quote! { ferriorm_runtime::filter::StringFilter },
        ScalarType::Int => quote! { ferriorm_runtime::filter::IntFilter },
        ScalarType::BigInt => quote! { ferriorm_runtime::filter::BigIntFilter },
        ScalarType::Float => quote! { ferriorm_runtime::filter::FloatFilter },
        ScalarType::Boolean => quote! { ferriorm_runtime::filter::BoolFilter },
        ScalarType::DateTime => quote! { ferriorm_runtime::filter::DateTimeFilter },
        ScalarType::Json | ScalarType::Bytes | ScalarType::Decimal => return None,
    };
    Some(tokens)
}

fn nullable_scalar_filter_type(scalar: &ScalarType) -> Option<TokenStream> {
    let tokens = match scalar {
        ScalarType::String => quote! { ferriorm_runtime::filter::NullableStringFilter },
        ScalarType::Int => quote! { ferriorm_runtime::filter::NullableIntFilter },
        ScalarType::BigInt => quote! { ferriorm_runtime::filter::NullableBigIntFilter },
        ScalarType::Float => quote! { ferriorm_runtime::filter::NullableFloatFilter },
        ScalarType::Boolean => quote! { ferriorm_runtime::filter::NullableBoolFilter },
        ScalarType::DateTime => quote! { ferriorm_runtime::filter::NullableDateTimeFilter },
        ScalarType::Json | ScalarType::Bytes | ScalarType::Decimal => return None,
    };
    Some(tokens)
}
