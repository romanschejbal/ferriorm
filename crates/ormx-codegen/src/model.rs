use ormx_core::schema::{Field, FieldKind, Model};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::rust_type::{filter_type_tokens, rust_type_tokens, ModuleDepth};

/// Generate the complete module for a single model.
pub fn generate_model_module(model: &Model) -> TokenStream {
    let scalar_fields: Vec<&Field> = model.fields.iter().filter(|f| f.is_scalar()).collect();

    let data_struct = generate_data_struct(model, &scalar_fields);
    let filter_module = generate_filter_module(model, &scalar_fields);
    let data_module = generate_data_module(model, &scalar_fields);
    let order_module = generate_order_module(model, &scalar_fields);
    let actions_struct = generate_actions_struct(model);
    let query_builders = generate_query_builders(model);

    quote! {
        #![allow(unused_imports, dead_code, clippy::all)]

        use serde::{Deserialize, Serialize};
        use ormx_runtime::prelude::*;

        #data_struct
        #filter_module
        #data_module
        #order_module
        #actions_struct
        #query_builders
    }
}

/// Generate the main data struct with sqlx::FromRow.
fn generate_data_struct(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let struct_name = format_ident!("{}", model.name);
    let table_name = &model.db_name;

    let fields: Vec<TokenStream> = scalar_fields
        .iter()
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let ty = rust_type_tokens(f, ModuleDepth::TopLevel);
            let db_name = &f.db_name;

            if db_name != &to_snake_case(&f.name) {
                quote! { #[sqlx(rename = #db_name)] pub #name: #ty }
            } else {
                quote! { pub #name: #ty }
            }
        })
        .collect();

    quote! {
        #[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
        #[sqlx(rename_all = "snake_case")]
        pub struct #struct_name {
            #(#fields),*
        }

        impl #struct_name {
            pub const TABLE_NAME: &'static str = #table_name;
        }
    }
}

/// Generate the filter module with WhereInput and WhereUniqueInput.
fn generate_filter_module(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let model_name = &model.name;
    let where_input_name = format_ident!("{}WhereInput", model_name);
    let where_unique_name = format_ident!("{}WhereUniqueInput", model_name);

    // WhereInput fields (using Nested depth since we're inside a submodule)
    let where_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter_map(|f| {
            let filter_ty = filter_type_tokens(f, ModuleDepth::Nested)?;
            let name = format_ident!("{}", to_snake_case(&f.name));
            Some(quote! { pub #name: Option<#filter_ty> })
        })
        .collect();

    // WhereUniqueInput variants (PascalCase)
    let unique_variants: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| f.is_id || f.is_unique)
        .map(|f| {
            let variant_name = format_ident!("{}", to_pascal_case(&f.name));
            let ty = rust_type_tokens(f, ModuleDepth::Nested);
            quote! { #variant_name(#ty) }
        })
        .collect();

    // WhereClause implementation
    let where_clause_arms: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| filter_type_tokens(f, ModuleDepth::Nested).is_some())
        .map(|f| {
            let field_name = format_ident!("{}", to_snake_case(&f.name));
            let db_name = &f.db_name;
            generate_filter_apply(f, &field_name, db_name)
        })
        .collect();

    // UniqueWhereClause implementation
    let unique_clause_arms: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| f.is_id || f.is_unique)
        .map(|f| {
            let variant_name = format_ident!("{}", to_pascal_case(&f.name));
            let db_name = &f.db_name;
            quote! {
                #where_unique_name::#variant_name(_) => {
                    builder.push(" AND ");
                    builder.push_identifier(#db_name);
                    builder.push(" = ");
                    builder.push_param();
                }
            }
        })
        .collect();

    quote! {
        pub mod filter {
            use ormx_runtime::prelude::*;

            #[derive(Debug, Clone, Default)]
            pub struct #where_input_name {
                #(#where_fields,)*
                pub and: Option<Vec<#where_input_name>>,
                pub or: Option<Vec<#where_input_name>>,
                pub not: Option<Box<#where_input_name>>,
            }

            #[derive(Debug, Clone)]
            pub enum #where_unique_name {
                #(#unique_variants),*
            }

            impl WhereClause for #where_input_name {
                fn apply_to(&self, builder: &mut SqlBuilder) {
                    #(#where_clause_arms)*

                    if let Some(and_conditions) = &self.and {
                        for condition in and_conditions {
                            condition.apply_to(builder);
                        }
                    }

                    if let Some(or_conditions) = &self.or {
                        if !or_conditions.is_empty() {
                            builder.push(" AND (");
                            for (i, condition) in or_conditions.iter().enumerate() {
                                if i > 0 {
                                    builder.push(" OR ");
                                }
                                builder.push("(1=1");
                                condition.apply_to(builder);
                                builder.push(")");
                            }
                            builder.push(")");
                        }
                    }

                    if let Some(not_condition) = &self.not {
                        builder.push(" AND NOT (1=1");
                        not_condition.apply_to(builder);
                        builder.push(")");
                    }
                }
            }

            impl UniqueWhereClause for #where_unique_name {
                fn apply_to(&self, builder: &mut SqlBuilder) {
                    match self {
                        #(#unique_clause_arms)*
                    }
                }
            }
        }
    }
}

/// Generate filter application code for a single field.
fn generate_filter_apply(
    field: &Field,
    field_ident: &proc_macro2::Ident,
    db_name: &str,
) -> TokenStream {
    let is_string = matches!(
        &field.field_type,
        FieldKind::Scalar(s) if matches!(s, ormx_core::types::ScalarType::String)
    );
    let is_comparable = matches!(
        &field.field_type,
        FieldKind::Scalar(s) if matches!(
            s,
            ormx_core::types::ScalarType::Int
                | ormx_core::types::ScalarType::BigInt
                | ormx_core::types::ScalarType::Float
                | ormx_core::types::ScalarType::DateTime
        )
    );

    let mut arms = vec![];

    arms.push(quote! {
        if filter.equals.is_some() {
            builder.push(" AND ");
            builder.push_identifier(#db_name);
            builder.push(" = ");
            builder.push_param();
        }
    });

    arms.push(quote! {
        if filter.not.is_some() {
            builder.push(" AND ");
            builder.push_identifier(#db_name);
            builder.push(" != ");
            builder.push_param();
        }
    });

    if is_string {
        arms.push(quote! {
            if filter.contains.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" LIKE ");
                builder.push_param();
            }
            if filter.starts_with.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" LIKE ");
                builder.push_param();
            }
            if filter.ends_with.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" LIKE ");
                builder.push_param();
            }
        });
    }

    if is_comparable {
        arms.push(quote! {
            if filter.gt.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" > ");
                builder.push_param();
            }
            if filter.gte.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" >= ");
                builder.push_param();
            }
            if filter.lt.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" < ");
                builder.push_param();
            }
            if filter.lte.is_some() {
                builder.push(" AND ");
                builder.push_identifier(#db_name);
                builder.push(" <= ");
                builder.push_param();
            }
        });
    }

    quote! {
        if let Some(filter) = &self.#field_ident {
            #(#arms)*
        }
    }
}

/// Generate the data module with CreateInput and UpdateInput.
fn generate_data_module(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let model_name = &model.name;
    let create_name = format_ident!("{}CreateInput", model_name);
    let update_name = format_ident!("{}UpdateInput", model_name);

    // Required fields: scalar fields that DON'T have a default and are NOT @updatedAt
    let required_create_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| !f.has_default() && !f.is_updated_at)
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let ty = rust_type_tokens(f, ModuleDepth::Nested);
            quote! { pub #name: #ty }
        })
        .collect();

    // Optional fields: scalar fields that HAVE a default (user can override)
    // Exclude @updatedAt since it's always auto-set
    let optional_create_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| f.has_default() && !f.is_updated_at)
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let base_ty = rust_type_tokens(f, ModuleDepth::Nested);
            // Wrap in Option so the user can optionally override the default
            quote! { pub #name: Option<#base_ty> }
        })
        .collect();

    // Update input: all non-id, non-updatedAt fields wrapped in Option<SetValue<T>>
    let update_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| !f.is_id && !f.is_updated_at)
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let ty = rust_type_tokens(f, ModuleDepth::Nested);
            quote! { pub #name: Option<SetValue<#ty>> }
        })
        .collect();

    quote! {
        pub mod data {
            use ormx_runtime::prelude::*;

            #[derive(Debug, Clone)]
            pub struct #create_name {
                #(#required_create_fields,)*
                #(#optional_create_fields,)*
            }

            #[derive(Debug, Clone, Default)]
            pub struct #update_name {
                #(#update_fields,)*
            }
        }
    }
}

/// Generate the order module with OrderByInput.
fn generate_order_module(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let model_name = &model.name;
    let order_name = format_ident!("{}OrderByInput", model_name);

    let variants: Vec<TokenStream> = scalar_fields
        .iter()
        .map(|f| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            quote! { #variant(SortOrder) }
        })
        .collect();

    let order_arms: Vec<TokenStream> = scalar_fields
        .iter()
        .map(|f| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            let db_name = &f.db_name;
            quote! {
                #order_name::#variant(order) => {
                    builder.push_identifier(#db_name);
                    builder.push(" ");
                    builder.push(order.as_sql());
                }
            }
        })
        .collect();

    quote! {
        pub mod order {
            use ormx_runtime::prelude::*;

            #[derive(Debug, Clone)]
            pub enum #order_name {
                #(#variants),*
            }

            impl OrderByClause for #order_name {
                fn apply_to(&self, builder: &mut SqlBuilder) {
                    match self {
                        #(#order_arms)*
                    }
                }
            }
        }
    }
}

/// Generate the Actions struct with CRUD methods.
fn generate_actions_struct(model: &Model) -> TokenStream {
    let model_name = format_ident!("{}", model.name);
    let actions_name = format_ident!("{}Actions", model.name);
    let where_input = format_ident!("{}WhereInput", model.name);
    let where_unique = format_ident!("{}WhereUniqueInput", model.name);
    let create_input = format_ident!("{}CreateInput", model.name);
    let update_input = format_ident!("{}UpdateInput", model.name);
    let order_by = format_ident!("{}OrderByInput", model.name);

    quote! {
        pub struct #actions_name<'a> {
            client: &'a DatabaseClient,
        }

        impl<'a> #actions_name<'a> {
            pub fn new(client: &'a DatabaseClient) -> Self {
                Self { client }
            }

            pub fn find_unique(
                &self,
                r#where: filter::#where_unique,
            ) -> FindUniqueQuery<'a, #model_name, filter::#where_unique> {
                FindUniqueQuery::new(self.client, r#where)
            }

            pub fn find_first(
                &self,
                r#where: filter::#where_input,
            ) -> FindFirstQuery<'a, #model_name, filter::#where_input, order::#order_by> {
                FindFirstQuery::new(self.client, r#where)
            }

            pub fn find_many(
                &self,
                r#where: filter::#where_input,
            ) -> FindManyQuery<'a, #model_name, filter::#where_input, order::#order_by> {
                FindManyQuery::new(self.client, r#where)
            }

            pub fn create(
                &self,
                data: data::#create_input,
            ) -> CreateQuery<'a, #model_name, data::#create_input> {
                CreateQuery::new(self.client, data)
            }

            pub fn update(
                &self,
                r#where: filter::#where_unique,
                data: data::#update_input,
            ) -> UpdateQuery<'a, #model_name, filter::#where_unique, data::#update_input> {
                UpdateQuery::new(self.client, r#where, data)
            }

            pub fn delete(
                &self,
                r#where: filter::#where_unique,
            ) -> DeleteQuery<'a, #model_name, filter::#where_unique> {
                DeleteQuery::new(self.client, r#where)
            }

            pub fn upsert(
                &self,
                r#where: filter::#where_unique,
                create: data::#create_input,
                update: data::#update_input,
            ) -> UpsertQuery<'a, #model_name, filter::#where_unique, data::#create_input, data::#update_input> {
                UpsertQuery::new(self.client, r#where, create, update)
            }

            pub fn create_many(
                &self,
                data: Vec<data::#create_input>,
            ) -> CreateManyQuery<'a, data::#create_input> {
                CreateManyQuery::new(self.client, data)
            }

            pub fn update_many(
                &self,
                r#where: filter::#where_input,
                data: data::#update_input,
            ) -> UpdateManyQuery<'a, filter::#where_input, data::#update_input> {
                UpdateManyQuery::new(self.client, r#where, data)
            }

            pub fn delete_many(
                &self,
                r#where: filter::#where_input,
            ) -> DeleteManyQuery<'a, filter::#where_input> {
                DeleteManyQuery::new(self.client, r#where)
            }

            pub fn count(
                &self,
                r#where: filter::#where_input,
            ) -> CountQuery<'a, filter::#where_input> {
                CountQuery::new(self.client, r#where)
            }
        }
    }
}

/// Generate the fluent query builder structs.
fn generate_query_builders(model: &Model) -> TokenStream {
    let _model_name = format_ident!("{}", model.name);

    quote! {
        pub struct FindUniqueQuery<'a, T, W> {
            client: &'a DatabaseClient,
            r#where: W,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, W: UniqueWhereClause> FindUniqueQuery<'a, T, W> {
            pub fn new(client: &'a DatabaseClient, r#where: W) -> Self {
                Self { client, r#where, _marker: std::marker::PhantomData }
            }
        }

        pub struct FindFirstQuery<'a, T, W, O> {
            client: &'a DatabaseClient,
            r#where: W,
            order_by: Vec<O>,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, W: WhereClause, O: OrderByClause> FindFirstQuery<'a, T, W, O> {
            pub fn new(client: &'a DatabaseClient, r#where: W) -> Self {
                Self { client, r#where, order_by: vec![], _marker: std::marker::PhantomData }
            }

            pub fn order_by(mut self, order: O) -> Self {
                self.order_by.push(order);
                self
            }
        }

        pub struct FindManyQuery<'a, T, W, O> {
            client: &'a DatabaseClient,
            r#where: W,
            order_by: Vec<O>,
            skip: Option<i64>,
            take: Option<i64>,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, W: WhereClause, O: OrderByClause> FindManyQuery<'a, T, W, O> {
            pub fn new(client: &'a DatabaseClient, r#where: W) -> Self {
                Self {
                    client, r#where, order_by: vec![],
                    skip: None, take: None,
                    _marker: std::marker::PhantomData,
                }
            }

            pub fn order_by(mut self, order: O) -> Self {
                self.order_by.push(order);
                self
            }

            pub fn skip(mut self, skip: i64) -> Self {
                self.skip = Some(skip);
                self
            }

            pub fn take(mut self, take: i64) -> Self {
                self.take = Some(take);
                self
            }
        }

        pub struct CreateQuery<'a, T, D> {
            client: &'a DatabaseClient,
            data: D,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, D> CreateQuery<'a, T, D> {
            pub fn new(client: &'a DatabaseClient, data: D) -> Self {
                Self { client, data, _marker: std::marker::PhantomData }
            }
        }

        pub struct UpdateQuery<'a, T, W, D> {
            client: &'a DatabaseClient,
            r#where: W,
            data: D,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, W, D> UpdateQuery<'a, T, W, D> {
            pub fn new(client: &'a DatabaseClient, r#where: W, data: D) -> Self {
                Self { client, r#where, data, _marker: std::marker::PhantomData }
            }
        }

        pub struct DeleteQuery<'a, T, W> {
            client: &'a DatabaseClient,
            r#where: W,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, W> DeleteQuery<'a, T, W> {
            pub fn new(client: &'a DatabaseClient, r#where: W) -> Self {
                Self { client, r#where, _marker: std::marker::PhantomData }
            }
        }

        pub struct UpsertQuery<'a, T, W, C, U> {
            client: &'a DatabaseClient,
            r#where: W,
            create: C,
            update: U,
            _marker: std::marker::PhantomData<T>,
        }

        impl<'a, T, W, C, U> UpsertQuery<'a, T, W, C, U> {
            pub fn new(client: &'a DatabaseClient, r#where: W, create: C, update: U) -> Self {
                Self { client, r#where, create, update, _marker: std::marker::PhantomData }
            }
        }

        pub struct CreateManyQuery<'a, D> {
            client: &'a DatabaseClient,
            data: Vec<D>,
        }

        impl<'a, D> CreateManyQuery<'a, D> {
            pub fn new(client: &'a DatabaseClient, data: Vec<D>) -> Self {
                Self { client, data }
            }
        }

        pub struct UpdateManyQuery<'a, W, D> {
            client: &'a DatabaseClient,
            r#where: W,
            data: D,
        }

        impl<'a, W, D> UpdateManyQuery<'a, W, D> {
            pub fn new(client: &'a DatabaseClient, r#where: W, data: D) -> Self {
                Self { client, r#where, data }
            }
        }

        pub struct DeleteManyQuery<'a, W> {
            client: &'a DatabaseClient,
            r#where: W,
        }

        impl<'a, W> DeleteManyQuery<'a, W> {
            pub fn new(client: &'a DatabaseClient, r#where: W) -> Self {
                Self { client, r#where }
            }
        }

        pub struct CountQuery<'a, W> {
            client: &'a DatabaseClient,
            r#where: W,
        }

        impl<'a, W> CountQuery<'a, W> {
            pub fn new(client: &'a DatabaseClient, r#where: W) -> Self {
                Self { client, r#where }
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

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}
