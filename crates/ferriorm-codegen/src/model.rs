//! Generates per-model Rust modules (struct, filters, data inputs, ordering, CRUD).
//!
//! For each model in the schema, this module produces:
//!
//! - A **data struct** (e.g., `User`) with `sqlx::FromRow` and serde derives.
//! - A **filter submodule** with `WhereInput` and `WhereUniqueInput` types.
//! - A **data submodule** with `CreateInput` and `UpdateInput` types.
//! - An **order submodule** with `OrderByInput`.
//! - An **`Actions` struct** exposing `create`, `find_unique`, `find_many`,
//!   `update`, `delete`, `upsert`, and batch operations.
//! - **Query builder structs** that chain filters, ordering, pagination, and
//!   include clauses before calling `.exec()`.

use ferriorm_core::schema::{Field, FieldKind, Model};
use ferriorm_core::types::ScalarType;
use ferriorm_core::utils::{to_pascal_case, to_snake_case};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::rust_type::{ModuleDepth, filter_type_tokens, rust_type_tokens};

/// Generate the complete module for a single model.
pub fn generate_model_module(model: &Model) -> TokenStream {
    let scalar_fields: Vec<&Field> = model.fields.iter().filter(|f| f.is_scalar()).collect();

    let data_struct = gen_data_struct(model, &scalar_fields);
    let filter_module = gen_filter_module(model, &scalar_fields);
    let data_module = gen_data_module(model, &scalar_fields);
    let order_module = gen_order_module(model, &scalar_fields);
    let actions_struct = gen_actions(model, &scalar_fields);
    let query_builders = gen_query_builders(model, &scalar_fields);
    let aggregate_types = gen_aggregate_types(model, &scalar_fields);
    let select_types = gen_select_types(model, &scalar_fields);

    quote! {
        #![allow(unused_imports, dead_code, clippy::all, unused_variables)]

        use serde::{Deserialize, Serialize};
        use ferriorm_runtime::prelude::*;

        #data_struct
        #filter_module
        #data_module
        #order_module
        #actions_struct
        #query_builders
        #aggregate_types
        #select_types
    }
}

// ─── Data Struct ──────────────────────────────────────────────

fn gen_data_struct(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
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

// ─── Filter Module ────────────────────────────────────────────

fn gen_filter_module(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let where_input = format_ident!("{}WhereInput", model.name);
    let where_unique = format_ident!("{}WhereUniqueInput", model.name);

    let where_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter_map(|f| {
            let filter_ty = filter_type_tokens(f, ModuleDepth::Nested)?;
            let name = format_ident!("{}", to_snake_case(&f.name));
            Some(quote! { pub #name: Option<#filter_ty> })
        })
        .collect();

    let unique_variants: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| f.is_id || f.is_unique)
        .map(|f| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            let ty = rust_type_tokens(f, ModuleDepth::Nested);
            quote! { #variant(#ty) }
        })
        .collect();

    // Generate build_where for WhereInput
    let db_bounds = collect_db_bounds(scalar_fields);
    let where_arms = gen_where_arms(scalar_fields);
    let unique_arms = gen_unique_where_arms(scalar_fields);

    quote! {
        pub mod filter {
            use ferriorm_runtime::prelude::*;

            #[derive(Debug, Clone, Default)]
            pub struct #where_input {
                #(#where_fields,)*
                pub and: Option<Vec<#where_input>>,
                pub or: Option<Vec<#where_input>>,
                pub not: Option<Box<#where_input>>,
            }

            #[derive(Debug, Clone)]
            pub enum #where_unique {
                #(#unique_variants),*
            }

            impl #where_input {
                pub(crate) fn build_where<'args, DB: sqlx::Database>(
                    &self,
                    qb: &mut sqlx::QueryBuilder<'args, DB>,
                )
                where
                    #(#db_bounds,)*
                {
                    #(#where_arms)*

                    if let Some(conditions) = &self.and {
                        for c in conditions {
                            c.build_where(qb);
                        }
                    }
                    if let Some(conditions) = &self.or {
                        if !conditions.is_empty() {
                            qb.push(" AND (");
                            for (i, c) in conditions.iter().enumerate() {
                                if i > 0 { qb.push(" OR "); }
                                qb.push("(1=1");
                                c.build_where(qb);
                                qb.push(")");
                            }
                            qb.push(")");
                        }
                    }
                    if let Some(c) = &self.not {
                        qb.push(" AND NOT (1=1");
                        c.build_where(qb);
                        qb.push(")");
                    }
                }
            }

            impl #where_unique {
                pub(crate) fn build_where<'args, DB: sqlx::Database>(
                    &self,
                    qb: &mut sqlx::QueryBuilder<'args, DB>,
                )
                where
                    #(#db_bounds,)*
                {
                    match self {
                        #(#unique_arms)*
                    }
                }
            }
        }
    }
}

/// Collect the sqlx type bounds needed for all scalar types used by the model.
fn collect_db_bounds(scalar_fields: &[&Field]) -> Vec<TokenStream> {
    let mut seen = std::collections::HashSet::new();
    let mut bounds = Vec::new();

    // Always need i64 for LIMIT/OFFSET
    seen.insert("i64");
    bounds.push(quote! { i64: sqlx::Type<DB> + for<'e> sqlx::Encode<'e, DB> });

    for f in scalar_fields {
        match &f.field_type {
            FieldKind::Scalar(scalar) => {
                let key = scalar.rust_type();
                if seen.insert(key)
                    && let Some(ty) = scalar_bound_tokens(scalar)
                {
                    bounds.push(quote! { #ty: sqlx::Type<DB> + for<'e> sqlx::Encode<'e, DB> });
                    // Also add Option<T> bound for nullable field support
                    bounds.push(
                        quote! { Option<#ty>: sqlx::Type<DB> + for<'e> sqlx::Encode<'e, DB> },
                    );
                }
            }
            FieldKind::Enum(_) => {}
            _ => {}
        }
    }

    bounds
}

fn scalar_bound_tokens(scalar: &ScalarType) -> Option<TokenStream> {
    match scalar {
        ScalarType::String => Some(quote! { String }),
        ScalarType::Int => Some(quote! { i32 }),
        ScalarType::BigInt => Some(quote! { i64 }),
        ScalarType::Float => Some(quote! { f64 }),
        ScalarType::Boolean => Some(quote! { bool }),
        ScalarType::DateTime => Some(quote! { chrono::DateTime<chrono::Utc> }),
        ScalarType::Bytes => Some(quote! { Vec<u8> }),
        ScalarType::Json | ScalarType::Decimal => None,
    }
}

/// Generate where-clause arms for each filterable scalar field.
fn gen_where_arms(scalar_fields: &[&Field]) -> Vec<TokenStream> {
    scalar_fields
        .iter()
        .filter_map(|f| {
            // Only generate filter arms for scalar types (skip enums for now)
            if !matches!(&f.field_type, FieldKind::Scalar(_)) {
                return None;
            }
            let field_ident = format_ident!("{}", to_snake_case(&f.name));
            let db_name = &f.db_name;
            let is_string = matches!(&f.field_type, FieldKind::Scalar(ScalarType::String));
            let is_comparable = matches!(
                &f.field_type,
                FieldKind::Scalar(
                    ScalarType::Int | ScalarType::BigInt | ScalarType::Float | ScalarType::DateTime
                )
            );

            let mut arms = vec![];

            arms.push(quote! {
                if let Some(v) = &filter.equals {
                    qb.push(concat!(" AND \"", #db_name, "\" = "));
                    qb.push_bind(v.clone());
                }
                if let Some(v) = &filter.not {
                    qb.push(concat!(" AND \"", #db_name, "\" != "));
                    qb.push_bind(v.clone());
                }
            });

            if is_string {
                arms.push(quote! {
                    if let Some(v) = &filter.contains {
                        qb.push(concat!(" AND \"", #db_name, "\" LIKE "));
                        qb.push_bind(format!("%{}%", v));
                    }
                    if let Some(v) = &filter.starts_with {
                        qb.push(concat!(" AND \"", #db_name, "\" LIKE "));
                        qb.push_bind(format!("{}%", v));
                    }
                    if let Some(v) = &filter.ends_with {
                        qb.push(concat!(" AND \"", #db_name, "\" LIKE "));
                        qb.push_bind(format!("%{}", v));
                    }
                });
            }

            if is_comparable {
                arms.push(quote! {
                    if let Some(v) = &filter.gt {
                        qb.push(concat!(" AND \"", #db_name, "\" > "));
                        qb.push_bind(v.clone());
                    }
                    if let Some(v) = &filter.gte {
                        qb.push(concat!(" AND \"", #db_name, "\" >= "));
                        qb.push_bind(v.clone());
                    }
                    if let Some(v) = &filter.lt {
                        qb.push(concat!(" AND \"", #db_name, "\" < "));
                        qb.push_bind(v.clone());
                    }
                    if let Some(v) = &filter.lte {
                        qb.push(concat!(" AND \"", #db_name, "\" <= "));
                        qb.push_bind(v.clone());
                    }
                });
            }

            Some(quote! {
                if let Some(filter) = &self.#field_ident {
                    #(#arms)*
                }
            })
        })
        .collect()
}

fn gen_unique_where_arms(scalar_fields: &[&Field]) -> Vec<TokenStream> {
    let _where_unique = format_ident!(
        "{}WhereUniqueInput",
        "" // placeholder, we use Self:: instead
    );
    scalar_fields
        .iter()
        .filter(|f| f.is_id || f.is_unique)
        .map(|f| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            let db_name = &f.db_name;
            quote! {
                Self::#variant(v) => {
                    qb.push(concat!(" AND \"", #db_name, "\" = "));
                    qb.push_bind(v.clone());
                }
            }
        })
        .collect()
}

// ─── Data Module ──────────────────────────────────────────────

fn gen_data_module(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let create_name = format_ident!("{}CreateInput", model.name);
    let update_name = format_ident!("{}UpdateInput", model.name);

    let required_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| !f.has_default() && !f.is_updated_at)
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let ty = rust_type_tokens(f, ModuleDepth::Nested);
            quote! { pub #name: #ty }
        })
        .collect();

    let optional_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .filter(|f| f.has_default() && !f.is_updated_at)
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let base_ty = rust_type_tokens(f, ModuleDepth::Nested);
            quote! { pub #name: Option<#base_ty> }
        })
        .collect();

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
            use ferriorm_runtime::prelude::*;

            #[derive(Debug, Clone)]
            pub struct #create_name {
                #(#required_fields,)*
                #(#optional_fields,)*
            }

            #[derive(Debug, Clone, Default)]
            pub struct #update_name {
                #(#update_fields,)*
            }
        }
    }
}

// ─── Order Module ─────────────────────────────────────────────

fn gen_order_module(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let order_name = format_ident!("{}OrderByInput", model.name);

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
                Self::#variant(order) => {
                    qb.push(concat!("\"", #db_name, "\" "));
                    qb.push(order.as_sql());
                }
            }
        })
        .collect();

    quote! {
        pub mod order {
            use ferriorm_runtime::prelude::*;

            #[derive(Debug, Clone)]
            pub enum #order_name {
                #(#variants),*
            }

            impl #order_name {
                pub(crate) fn build_order_by<'args, DB: sqlx::Database>(
                    &self,
                    qb: &mut sqlx::QueryBuilder<'args, DB>,
                ) {
                    match self {
                        #(#order_arms)*
                    }
                }
            }
        }
    }
}

// ─── Actions ──────────────────────────────────────────────────

fn gen_actions(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let _model_ident = format_ident!("{}", model.name);
    let actions_name = format_ident!("{}Actions", model.name);
    let where_input = format_ident!("{}WhereInput", model.name);
    let where_unique = format_ident!("{}WhereUniqueInput", model.name);
    let create_input = format_ident!("{}CreateInput", model.name);
    let update_input = format_ident!("{}UpdateInput", model.name);
    let _order_by = format_ident!("{}OrderByInput", model.name);

    // Only generate aggregate() if there are aggregatable fields
    let has_agg_fields = scalar_fields.iter().any(|f| {
        matches!(
            &f.field_type,
            FieldKind::Scalar(
                ScalarType::Int | ScalarType::BigInt | ScalarType::Float | ScalarType::DateTime
            )
        )
    });
    let aggregate_method = if has_agg_fields {
        quote! {
            pub fn aggregate(&self, r#where: filter::#where_input) -> AggregateQuery<'a> {
                AggregateQuery { client: self.client, r#where, ops: vec![] }
            }
        }
    } else {
        quote! {}
    };

    quote! {
        pub struct #actions_name<'a> {
            client: &'a DatabaseClient,
        }

        impl<'a> #actions_name<'a> {
            pub fn new(client: &'a DatabaseClient) -> Self { Self { client } }

            pub fn find_unique(&self, r#where: filter::#where_unique) -> FindUniqueQuery<'a> {
                FindUniqueQuery { client: self.client, r#where }
            }

            pub fn find_first(&self, r#where: filter::#where_input) -> FindFirstQuery<'a> {
                FindFirstQuery { client: self.client, r#where, order_by: vec![] }
            }

            pub fn find_many(&self, r#where: filter::#where_input) -> FindManyQuery<'a> {
                FindManyQuery { client: self.client, r#where, order_by: vec![], skip: None, take: None }
            }

            pub fn create(&self, data: data::#create_input) -> CreateQuery<'a> {
                CreateQuery { client: self.client, data }
            }

            pub fn update(&self, r#where: filter::#where_unique, data: data::#update_input) -> UpdateQuery<'a> {
                UpdateQuery { client: self.client, r#where, data }
            }

            pub fn delete(&self, r#where: filter::#where_unique) -> DeleteQuery<'a> {
                DeleteQuery { client: self.client, r#where }
            }

            pub fn count(&self, r#where: filter::#where_input) -> CountQuery<'a> {
                CountQuery { client: self.client, r#where }
            }

            pub fn create_many(&self, data: Vec<data::#create_input>) -> CreateManyQuery<'a> {
                CreateManyQuery { client: self.client, data }
            }

            pub fn update_many(&self, r#where: filter::#where_input, data: data::#update_input) -> UpdateManyQuery<'a> {
                UpdateManyQuery { client: self.client, r#where, data }
            }

            pub fn delete_many(&self, r#where: filter::#where_input) -> DeleteManyQuery<'a> {
                DeleteManyQuery { client: self.client, r#where }
            }

            #aggregate_method
        }
    }
}

// ─── Query Builders with exec() ──────────────────────────────

fn gen_query_builders(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let model_ident = format_ident!("{}", model.name);
    let table_name = &model.db_name;
    let _where_input = format_ident!("{}WhereInput", model.name);
    let _where_unique = format_ident!("{}WhereUniqueInput", model.name);
    let _create_input = format_ident!("{}CreateInput", model.name);
    let _update_input = format_ident!("{}UpdateInput", model.name);
    let order_by = format_ident!("{}OrderByInput", model.name);
    let _select_struct = format_ident!("{}Select", model.name);
    let _partial_struct = format_ident!("{}Partial", model.name);
    let _aggregate_result = format_ident!("{}AggregateResult", model.name);
    let _aggregate_field = format_ident!("{}AggregateField", model.name);
    let db_bounds = collect_db_bounds(scalar_fields);

    let select_sql = format!(r#"SELECT * FROM "{}" WHERE 1=1"#, table_name);
    let count_sql = format!(
        r#"SELECT COUNT(*) as "count" FROM "{}" WHERE 1=1"#,
        table_name
    );
    let delete_sql = format!(r#"DELETE FROM "{}" WHERE 1=1"#, table_name);

    let insert_code = gen_insert_code(model, scalar_fields, table_name);
    let update_code = gen_update_code(model, scalar_fields, table_name);
    let update_many_code = gen_update_many_code(model, scalar_fields, table_name);

    quote! {
        // ── Generic helper: build ORDER BY clause ──────────────
        fn build_order_by<'args, DB: sqlx::Database>(
            orders: &[order::#order_by],
            qb: &mut sqlx::QueryBuilder<'args, DB>,
        ) {
            if !orders.is_empty() {
                qb.push(" ORDER BY ");
                for (i, ob) in orders.iter().enumerate() {
                    if i > 0 { qb.push(", "); }
                    ob.build_order_by(qb);
                }
            }
        }

        // ── Generic helper: build a SELECT query ───────────────
        fn build_select_query<'args, DB: sqlx::Database>(
            base_sql: &str,
            where_input: &filter::#_where_input,
            orders: &[order::#order_by],
            take: Option<i64>,
            skip: Option<i64>,
        ) -> sqlx::QueryBuilder<'args, DB>
        where
            #(#db_bounds,)*
        {
            let mut qb = sqlx::QueryBuilder::<DB>::new(base_sql);
            where_input.build_where(&mut qb);
            build_order_by(orders, &mut qb);
            if let Some(take) = take {
                qb.push(" LIMIT ");
                qb.push_bind(take);
            }
            if let Some(skip) = skip {
                qb.push(" OFFSET ");
                qb.push_bind(skip);
            }
            qb
        }

        // ── Generic helper: build a SELECT query for unique lookup ──
        fn build_unique_select_query<'args, DB: sqlx::Database>(
            base_sql: &str,
            where_unique: &filter::#_where_unique,
        ) -> sqlx::QueryBuilder<'args, DB>
        where
            #(#db_bounds,)*
        {
            let mut qb = sqlx::QueryBuilder::<DB>::new(base_sql);
            where_unique.build_where(&mut qb);
            qb.push(" LIMIT 1");
            qb
        }

        // ── Generic helper: build a DELETE-returning query ─────
        fn build_delete_query<'args, DB: sqlx::Database>(
            base_sql: &str,
            where_unique: &filter::#_where_unique,
        ) -> sqlx::QueryBuilder<'args, DB>
        where
            #(#db_bounds,)*
        {
            let mut qb = sqlx::QueryBuilder::<DB>::new(base_sql);
            where_unique.build_where(&mut qb);
            qb.push(" RETURNING *");
            qb
        }

        // ── Generic helper: build a COUNT query ────────────────
        fn build_count_query<'args, DB: sqlx::Database>(
            base_sql: &str,
            where_input: &filter::#_where_input,
        ) -> sqlx::QueryBuilder<'args, DB>
        where
            #(#db_bounds,)*
        {
            let mut qb = sqlx::QueryBuilder::<DB>::new(base_sql);
            where_input.build_where(&mut qb);
            qb
        }

        // ── Generic helper: build a DELETE-many query ──────────
        fn build_delete_many_query<'args, DB: sqlx::Database>(
            base_sql: &str,
            where_input: &filter::#_where_input,
        ) -> sqlx::QueryBuilder<'args, DB>
        where
            #(#db_bounds,)*
        {
            let mut qb = sqlx::QueryBuilder::<DB>::new(base_sql);
            where_input.build_where(&mut qb);
            qb
        }

        pub struct FindUniqueQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_unique,
        }

        impl<'a> FindUniqueQuery<'a> {
            pub fn select(self, select: #_select_struct) -> FindUniqueSelectQuery<'a> {
                FindUniqueSelectQuery { client: self.client, r#where: self.r#where, select }
            }

            pub async fn exec(self) -> Result<Option<#model_ident>, FerriormError> {
                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_unique_select_query::<sqlx::Postgres>(#select_sql, &self.r#where);
                        self.client.fetch_optional_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_unique_select_query::<sqlx::Sqlite>(#select_sql, &self.r#where);
                        self.client.fetch_optional_sqlite(qb).await
                    }
                }
            }
        }

        pub struct FindFirstQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
            order_by: Vec<order::#order_by>,
        }

        impl<'a> FindFirstQuery<'a> {
            pub fn order_by(mut self, order: order::#order_by) -> Self {
                self.order_by.push(order);
                self
            }

            pub fn select(self, select: #_select_struct) -> FindFirstSelectQuery<'a> {
                FindFirstSelectQuery { client: self.client, r#where: self.r#where, order_by: self.order_by, select }
            }

            pub async fn exec(self) -> Result<Option<#model_ident>, FerriormError> {
                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_select_query::<sqlx::Postgres>(#select_sql, &self.r#where, &self.order_by, Some(1), None);
                        self.client.fetch_optional_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_select_query::<sqlx::Sqlite>(#select_sql, &self.r#where, &self.order_by, Some(1), None);
                        self.client.fetch_optional_sqlite(qb).await
                    }
                }
            }
        }

        pub struct FindManyQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
            order_by: Vec<order::#order_by>,
            skip: Option<i64>,
            take: Option<i64>,
        }

        impl<'a> FindManyQuery<'a> {
            pub fn order_by(mut self, order: order::#order_by) -> Self {
                self.order_by.push(order);
                self
            }

            pub fn skip(mut self, n: i64) -> Self {
                self.skip = Some(n);
                self
            }

            pub fn take(mut self, n: i64) -> Self {
                self.take = Some(n);
                self
            }

            pub fn select(self, select: #_select_struct) -> FindManySelectQuery<'a> {
                FindManySelectQuery {
                    client: self.client,
                    r#where: self.r#where,
                    order_by: self.order_by,
                    skip: self.skip,
                    take: self.take,
                    select,
                }
            }

            pub async fn exec(self) -> Result<Vec<#model_ident>, FerriormError> {
                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_select_query::<sqlx::Postgres>(#select_sql, &self.r#where, &self.order_by, self.take, self.skip);
                        self.client.fetch_all_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_select_query::<sqlx::Sqlite>(#select_sql, &self.r#where, &self.order_by, self.take, self.skip);
                        self.client.fetch_all_sqlite(qb).await
                    }
                }
            }
        }

        pub struct CreateQuery<'a> {
            client: &'a DatabaseClient,
            data: data::#_create_input,
        }

        impl<'a> CreateQuery<'a> {
            pub async fn exec(self) -> Result<#model_ident, FerriormError> {
                let client = self.client;
                #insert_code
            }
        }

        pub struct UpdateQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_unique,
            data: data::#_update_input,
        }

        impl<'a> UpdateQuery<'a> {
            pub async fn exec(self) -> Result<#model_ident, FerriormError> {
                let client = self.client;
                #update_code
            }
        }

        pub struct DeleteQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_unique,
        }

        impl<'a> DeleteQuery<'a> {
            pub async fn exec(self) -> Result<#model_ident, FerriormError> {
                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_delete_query::<sqlx::Postgres>(#delete_sql, &self.r#where);
                        self.client.fetch_one_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_delete_query::<sqlx::Sqlite>(#delete_sql, &self.r#where);
                        self.client.fetch_one_sqlite(qb).await
                    }
                }
            }
        }

        #[derive(sqlx::FromRow)]
        struct CountResult { count: i64 }

        pub struct CountQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
        }

        impl<'a> CountQuery<'a> {
            pub async fn exec(self) -> Result<i64, FerriormError> {
                let row: CountResult = match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_count_query::<sqlx::Postgres>(#count_sql, &self.r#where);
                        self.client.fetch_one_pg(qb).await?
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_count_query::<sqlx::Sqlite>(#count_sql, &self.r#where);
                        self.client.fetch_one_sqlite(qb).await?
                    }
                };
                Ok(row.count)
            }
        }

        pub struct CreateManyQuery<'a> {
            client: &'a DatabaseClient,
            data: Vec<data::#_create_input>,
        }

        impl<'a> CreateManyQuery<'a> {
            pub async fn exec(self) -> Result<u64, FerriormError> {
                if self.data.is_empty() { return Ok(0); }
                let count = self.data.len() as u64;
                for item in self.data {
                    CreateQuery { client: self.client, data: item }.exec().await?;
                }
                Ok(count)
            }
        }

        pub struct UpdateManyQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
            data: data::#_update_input,
        }

        impl<'a> UpdateManyQuery<'a> {
            pub async fn exec(self) -> Result<u64, FerriormError> {
                let client = self.client;
                #update_many_code
            }
        }

        pub struct DeleteManyQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
        }

        impl<'a> DeleteManyQuery<'a> {
            pub async fn exec(self) -> Result<u64, FerriormError> {
                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_delete_many_query::<sqlx::Postgres>(#delete_sql, &self.r#where);
                        self.client.execute_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_delete_many_query::<sqlx::Sqlite>(#delete_sql, &self.r#where);
                        self.client.execute_sqlite(qb).await
                    }
                }
            }
        }
    }
}

// ─── INSERT code generation ───────────────────────────────────

fn gen_insert_code(model: &Model, scalar_fields: &[&Field], table_name: &str) -> TokenStream {
    let _model_ident = format_ident!("{}", model.name);

    // Required columns: scalar, no default, not @updatedAt
    let required: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| !f.has_default() && !f.is_updated_at)
        .collect();

    // Optional columns: have default (can be overridden), not @updatedAt
    let optional: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| f.has_default() && !f.is_updated_at)
        .collect();

    // @updatedAt columns: always set to now()
    let updated_at: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| f.is_updated_at)
        .collect();

    // Build column names and bind values
    let mut col_pushes = vec![];
    let mut val_pushes = vec![];

    // Required fields — always included
    for f in &required {
        let db_name = &f.db_name;
        let field_ident = format_ident!("{}", to_snake_case(&f.name));
        col_pushes.push(quote! { cols.push(#db_name); });
        val_pushes.push(quote! { sep.push_bind(self.data.#field_ident); });
    }

    // Optional fields — resolve defaults in Rust
    for f in &optional {
        let db_name = &f.db_name;
        let field_ident = format_ident!("{}", to_snake_case(&f.name));
        let default_expr = gen_default_expr(f);

        col_pushes.push(quote! { cols.push(#db_name); });
        val_pushes.push(quote! {
            let val = self.data.#field_ident.unwrap_or_else(|| #default_expr);
            sep.push_bind(val);
        });
    }

    // @updatedAt fields
    for f in &updated_at {
        let db_name = &f.db_name;
        col_pushes.push(quote! { cols.push(#db_name); });
        val_pushes.push(quote! { sep.push_bind(chrono::Utc::now()); });
    }

    let insert_start = format!(r#"INSERT INTO "{}""#, table_name);

    // The insert_body macro avoids duplicating the column/value building logic
    // for each database backend. It captures `self` by reference.
    quote! {
        // Helper to build the INSERT query for any DB backend
        macro_rules! build_insert {
            ($qb_type:ty) => {{
                let mut cols: Vec<&str> = Vec::new();
                #(#col_pushes)*

                let mut qb = sqlx::QueryBuilder::<$qb_type>::new(#insert_start);
                qb.push(" (");
                for (i, col) in cols.iter().enumerate() {
                    if i > 0 { qb.push(", "); }
                    qb.push("\"");
                    qb.push(*col);
                    qb.push("\"");
                }
                qb.push(") VALUES (");
                {
                    let mut sep = qb.separated(", ");
                    #(#val_pushes)*
                }
                qb.push(") RETURNING *");
                qb
            }};
        }

        match client {
            DatabaseClient::Postgres(_) => {
                let qb = build_insert!(sqlx::Postgres);
                client.fetch_one_pg(qb).await
            }
            DatabaseClient::Sqlite(_) => {
                let qb = build_insert!(sqlx::Sqlite);
                client.fetch_one_sqlite(qb).await
            }
        }
    }
}

/// Generate a Rust expression for a field's @default value.
fn gen_default_expr(field: &Field) -> TokenStream {
    use ferriorm_core::ast::DefaultValue;

    match &field.default {
        Some(DefaultValue::Uuid) => quote! { uuid::Uuid::new_v4().to_string() },
        Some(DefaultValue::Cuid) => quote! { uuid::Uuid::new_v4().to_string() }, // fallback
        Some(DefaultValue::Now) => quote! { chrono::Utc::now() },
        Some(DefaultValue::AutoIncrement) => quote! { 0 }, // DB handles this
        Some(DefaultValue::Literal(lit)) => {
            use ferriorm_core::ast::LiteralValue;
            match lit {
                LiteralValue::String(s) => quote! { #s.to_string() },
                LiteralValue::Int(i) => quote! { #i },
                LiteralValue::Float(f) => quote! { #f },
                LiteralValue::Bool(b) => quote! { #b },
            }
        }
        Some(DefaultValue::EnumVariant(v)) => {
            // Reference the enum variant — insert code runs at model module level
            let variant = format_ident!("{}", v);
            if let FieldKind::Enum(enum_name) = &field.field_type {
                let enum_ident = format_ident!("{}", enum_name);
                quote! { super::enums::#enum_ident::#variant }
            } else {
                quote! { Default::default() }
            }
        }
        None => quote! { Default::default() },
    }
}

// ─── UPDATE code generation ───────────────────────────────────

fn gen_update_code(model: &Model, scalar_fields: &[&Field], table_name: &str) -> TokenStream {
    let _model_ident = format_ident!("{}", model.name);

    // Updatable fields: non-id, non-updatedAt scalar fields
    let updatable: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| !f.is_id && !f.is_updated_at)
        .collect();

    let updated_at: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| f.is_updated_at)
        .collect();

    let update_start = format!(r#"UPDATE "{}" SET "#, table_name);

    // Generate SET clause arms
    let set_arms: Vec<TokenStream> = updatable
        .iter()
        .map(|f| {
            let field_ident = format_ident!("{}", to_snake_case(&f.name));
            let db_name = &f.db_name;
            quote! {
                if let Some(SetValue::Set(v)) = self.data.#field_ident {
                    if !first_set { qb.push(", "); }
                    first_set = false;
                    qb.push(concat!("\"", #db_name, "\" = "));
                    qb.push_bind(v);
                }
            }
        })
        .collect();

    let updated_at_arms: Vec<TokenStream> = updated_at
        .iter()
        .map(|f| {
            let db_name = &f.db_name;
            quote! {
                if !first_set { qb.push(", "); }
                first_set = false;
                qb.push(concat!("\"", #db_name, "\" = "));
                qb.push_bind(chrono::Utc::now());
            }
        })
        .collect();

    // The build_update macro avoids duplicating the SET clause building logic
    // for each database backend.
    quote! {
        macro_rules! build_update {
            ($qb_type:ty) => {{
                let mut qb = sqlx::QueryBuilder::<$qb_type>::new(#update_start);
                let mut first_set = true;
                #(#set_arms)*
                #(#updated_at_arms)*

                if first_set {
                    return Err(FerriormError::Query("No fields to update".into()));
                }

                qb.push(" WHERE 1=1");
                self.r#where.build_where(&mut qb);
                qb.push(" RETURNING *");
                qb
            }};
        }

        match client {
            DatabaseClient::Postgres(_) => {
                let qb = build_update!(sqlx::Postgres);
                client.fetch_one_pg(qb).await
            }
            DatabaseClient::Sqlite(_) => {
                let qb = build_update!(sqlx::Sqlite);
                client.fetch_one_sqlite(qb).await
            }
        }
    }
}

// ─── UPDATE MANY code generation ──────────────────────────────

fn gen_update_many_code(_model: &Model, scalar_fields: &[&Field], table_name: &str) -> TokenStream {
    // Updatable fields: non-id, non-updatedAt scalar fields
    let updatable: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| !f.is_id && !f.is_updated_at)
        .collect();

    let updated_at: Vec<&Field> = scalar_fields
        .iter()
        .copied()
        .filter(|f| f.is_updated_at)
        .collect();

    let update_start = format!(r#"UPDATE "{}" SET "#, table_name);

    // Generate SET clause arms
    let set_arms: Vec<TokenStream> = updatable
        .iter()
        .map(|f| {
            let field_ident = format_ident!("{}", to_snake_case(&f.name));
            let db_name = &f.db_name;
            quote! {
                if let Some(SetValue::Set(v)) = self.data.#field_ident {
                    if !first_set { qb.push(", "); }
                    first_set = false;
                    qb.push(concat!("\"", #db_name, "\" = "));
                    qb.push_bind(v);
                }
            }
        })
        .collect();

    let updated_at_arms: Vec<TokenStream> = updated_at
        .iter()
        .map(|f| {
            let db_name = &f.db_name;
            quote! {
                if !first_set { qb.push(", "); }
                first_set = false;
                qb.push(concat!("\"", #db_name, "\" = "));
                qb.push_bind(chrono::Utc::now());
            }
        })
        .collect();

    quote! {
        macro_rules! build_update_many {
            ($qb_type:ty) => {{
                let mut qb = sqlx::QueryBuilder::<$qb_type>::new(#update_start);
                let mut first_set = true;
                #(#set_arms)*
                #(#updated_at_arms)*

                if first_set {
                    return Ok(0);
                }

                qb.push(" WHERE 1=1");
                self.r#where.build_where(&mut qb);
                qb
            }};
        }

        match client {
            DatabaseClient::Postgres(_) => {
                let qb = build_update_many!(sqlx::Postgres);
                client.execute_pg(qb).await
            }
            DatabaseClient::Sqlite(_) => {
                let qb = build_update_many!(sqlx::Sqlite);
                client.execute_sqlite(qb).await
            }
        }
    }
}

// ─── Aggregate Types ──────────────────────────────────────────

/// Identifies which fields are aggregatable and what operations they support.
enum AggregateKind {
    /// Numeric fields: avg, sum, min, max
    Numeric,
    /// DateTime fields: min, max only
    DateTime,
}

fn gen_aggregate_types(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let aggregate_field_name = format_ident!("{}AggregateField", model.name);
    let aggregate_result_name = format_ident!("{}AggregateResult", model.name);
    let _where_input = format_ident!("{}WhereInput", model.name);
    let table_name = &model.db_name;

    // Collect aggregatable fields with their kind
    let agg_fields: Vec<(&Field, AggregateKind)> = scalar_fields
        .iter()
        .filter_map(|f| match &f.field_type {
            FieldKind::Scalar(ScalarType::Int | ScalarType::BigInt | ScalarType::Float) => {
                Some((*f, AggregateKind::Numeric))
            }
            FieldKind::Scalar(ScalarType::DateTime) => Some((*f, AggregateKind::DateTime)),
            _ => None,
        })
        .collect();

    if agg_fields.is_empty() {
        return quote! {};
    }

    // Generate enum variants
    let enum_variants: Vec<TokenStream> = agg_fields
        .iter()
        .map(|(f, _)| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            quote! { #variant }
        })
        .collect();

    // Generate db_name match arms
    let db_name_arms: Vec<TokenStream> = agg_fields
        .iter()
        .map(|(f, _)| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            let db_name = &f.db_name;
            quote! { Self::#variant => #db_name }
        })
        .collect();

    // Generate AggregateResult fields
    let mut result_fields = Vec::new();
    for (f, kind) in &agg_fields {
        let snake = to_snake_case(&f.name);
        let orig_ty = rust_type_tokens(
            &Field {
                is_optional: false,
                ..(*f).clone()
            },
            ModuleDepth::TopLevel,
        );

        match kind {
            AggregateKind::Numeric => {
                let avg_name = format_ident!("avg_{}", snake);
                let sum_name = format_ident!("sum_{}", snake);
                let min_name = format_ident!("min_{}", snake);
                let max_name = format_ident!("max_{}", snake);
                result_fields.push(quote! { #[sqlx(default)] pub #avg_name: Option<f64> });
                result_fields.push(quote! { #[sqlx(default)] pub #sum_name: Option<f64> });
                result_fields.push(quote! { #[sqlx(default)] pub #min_name: Option<#orig_ty> });
                result_fields.push(quote! { #[sqlx(default)] pub #max_name: Option<#orig_ty> });
            }
            AggregateKind::DateTime => {
                let min_name = format_ident!("min_{}", snake);
                let max_name = format_ident!("max_{}", snake);
                result_fields.push(quote! { #[sqlx(default)] pub #min_name: Option<#orig_ty> });
                result_fields.push(quote! { #[sqlx(default)] pub #max_name: Option<#orig_ty> });
            }
        }
    }

    // Generate the is_numeric check for avg/sum validation
    let numeric_arms: Vec<TokenStream> = agg_fields
        .iter()
        .filter(|(_, kind)| matches!(kind, AggregateKind::Numeric))
        .map(|(f, _)| {
            let variant = format_ident!("{}", to_pascal_case(&f.name));
            quote! { Self::#variant => true }
        })
        .collect();

    let has_numeric = !numeric_arms.is_empty();
    let is_numeric_method = if has_numeric {
        quote! {
            fn is_numeric(&self) -> bool {
                match self {
                    #(#numeric_arms,)*
                    #[allow(unreachable_patterns)]
                    _ => false,
                }
            }
        }
    } else {
        quote! {
            fn is_numeric(&self) -> bool { false }
        }
    };

    // Generate alias match arms for each (prefix, field) combination
    let mut alias_arms = Vec::new();
    for (f, kind) in &agg_fields {
        let variant = format_ident!("{}", to_pascal_case(&f.name));
        let snake = to_snake_case(&f.name);
        let prefixes = match kind {
            AggregateKind::Numeric => vec!["avg", "sum", "min", "max"],
            AggregateKind::DateTime => vec!["min", "max"],
        };
        for prefix in prefixes {
            let alias_str = format!("{}_{}", prefix, snake);
            alias_arms.push(quote! { (#prefix, Self::#variant) => #alias_str });
        }
    }

    let agg_select_base = format!(r#"SELECT {{}} FROM "{}" WHERE 1=1"#, table_name);

    quote! {
        #[derive(Debug, Clone, Copy)]
        pub enum #aggregate_field_name {
            #(#enum_variants),*
        }

        impl #aggregate_field_name {
            pub fn db_name(&self) -> &'static str {
                match self {
                    #(#db_name_arms,)*
                }
            }

            fn alias(&self, prefix: &'static str) -> &'static str {
                match (prefix, self) {
                    #(#alias_arms,)*
                    _ => unreachable!(),
                }
            }

            #is_numeric_method
        }

        #[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
        pub struct #aggregate_result_name {
            #(#result_fields,)*
        }

        pub struct AggregateQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
            ops: Vec<(&'static str, &'static str, &'static str)>,
        }

        impl<'a> AggregateQuery<'a> {
            pub fn avg(mut self, field: #aggregate_field_name) -> Self {
                assert!(field.is_numeric(), "avg() is only supported on numeric fields");
                let db_name = field.db_name();
                let alias = field.alias("avg");
                self.ops.push(("AVG", db_name, alias));
                self
            }

            pub fn sum(mut self, field: #aggregate_field_name) -> Self {
                assert!(field.is_numeric(), "sum() is only supported on numeric fields");
                let db_name = field.db_name();
                let alias = field.alias("sum");
                self.ops.push(("SUM", db_name, alias));
                self
            }

            pub fn min(mut self, field: #aggregate_field_name) -> Self {
                let db_name = field.db_name();
                let alias = field.alias("min");
                self.ops.push(("MIN", db_name, alias));
                self
            }

            pub fn max(mut self, field: #aggregate_field_name) -> Self {
                let db_name = field.db_name();
                let alias = field.alias("max");
                self.ops.push(("MAX", db_name, alias));
                self
            }

            pub async fn exec(self) -> Result<#aggregate_result_name, FerriormError> {
                if self.ops.is_empty() {
                    return Err(FerriormError::Query("No aggregate operations specified".into()));
                }

                let selections: Vec<String> = self.ops.iter()
                    .map(|(func, col, alias)| format!(r#"{}("{}") as "{}""#, func, col, alias))
                    .collect();
                let select_clause = selections.join(", ");
                let base_sql = format!(#agg_select_base, select_clause);

                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(&base_sql);
                        self.r#where.build_where(&mut qb);
                        self.client.fetch_one_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(&base_sql);
                        self.r#where.build_where(&mut qb);
                        self.client.fetch_one_sqlite(qb).await
                    }
                }
            }
        }
    }
}

// ─── Select Types ─────────────────────────────────────────────

fn gen_select_types(model: &Model, scalar_fields: &[&Field]) -> TokenStream {
    let select_name = format_ident!("{}Select", model.name);
    let partial_name = format_ident!("{}Partial", model.name);
    let _where_input = format_ident!("{}WhereInput", model.name);
    let _where_unique = format_ident!("{}WhereUniqueInput", model.name);
    let order_by_name = format_ident!("{}OrderByInput", model.name);
    let table_name = &model.db_name;

    // Select struct fields: all bool, default false
    let select_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            quote! { pub #name: bool }
        })
        .collect();

    // Partial struct fields: all Option<T> with #[sqlx(default)]
    // For already-optional fields, don't double-wrap in Option
    let partial_fields: Vec<TokenStream> = scalar_fields
        .iter()
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let db_name = &f.db_name;
            // Get the base type (non-optional version)
            let base_ty = rust_type_tokens(
                &Field {
                    is_optional: false,
                    ..(*f).clone()
                },
                ModuleDepth::TopLevel,
            );
            let rename = if db_name != &to_snake_case(&f.name) {
                quote! { #[sqlx(rename = #db_name)] }
            } else {
                quote! {}
            };
            // Always wrap in Option<T>, regardless of whether field was originally optional
            quote! { #[sqlx(default)] #rename pub #name: Option<#base_ty> }
        })
        .collect();

    // build_select_columns: maps Select bools to column names
    let select_col_arms: Vec<TokenStream> = scalar_fields
        .iter()
        .map(|f| {
            let name = format_ident!("{}", to_snake_case(&f.name));
            let db_name = &f.db_name;
            let col_expr = format!(r#""{}""#, db_name);
            quote! {
                if select.#name { cols.push(#col_expr); }
            }
        })
        .collect();

    let select_sql_prefix = format!(r#"SELECT {{}} FROM "{}" WHERE 1=1"#, table_name);

    quote! {
        #[derive(Debug, Clone, Default)]
        pub struct #select_name {
            #(#select_fields,)*
        }

        #[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
        #[sqlx(rename_all = "snake_case")]
        pub struct #partial_name {
            #(#partial_fields,)*
        }

        fn build_select_columns(select: &#select_name) -> String {
            let mut cols = Vec::new();
            #(#select_col_arms)*
            if cols.is_empty() {
                "*".to_string()
            } else {
                cols.join(", ")
            }
        }

        // ── FindManySelectQuery ──────────────────────────────────

        pub struct FindManySelectQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
            order_by: Vec<order::#order_by_name>,
            skip: Option<i64>,
            take: Option<i64>,
            select: #select_name,
        }

        impl<'a> FindManySelectQuery<'a> {
            pub fn order_by(mut self, order: order::#order_by_name) -> Self {
                self.order_by.push(order);
                self
            }

            pub fn skip(mut self, n: i64) -> Self {
                self.skip = Some(n);
                self
            }

            pub fn take(mut self, n: i64) -> Self {
                self.take = Some(n);
                self
            }

            pub async fn exec(self) -> Result<Vec<#partial_name>, FerriormError> {
                let cols = build_select_columns(&self.select);
                let base_sql = format!(#select_sql_prefix, cols);

                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_select_query::<sqlx::Postgres>(
                            &base_sql, &self.r#where, &self.order_by, self.take, self.skip,
                        );
                        self.client.fetch_all_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_select_query::<sqlx::Sqlite>(
                            &base_sql, &self.r#where, &self.order_by, self.take, self.skip,
                        );
                        self.client.fetch_all_sqlite(qb).await
                    }
                }
            }
        }

        // ── FindUniqueSelectQuery ────────────────────────────────

        pub struct FindUniqueSelectQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_unique,
            select: #select_name,
        }

        impl<'a> FindUniqueSelectQuery<'a> {
            pub async fn exec(self) -> Result<Option<#partial_name>, FerriormError> {
                let cols = build_select_columns(&self.select);
                let base_sql = format!(#select_sql_prefix, cols);

                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_unique_select_query::<sqlx::Postgres>(
                            &base_sql, &self.r#where,
                        );
                        self.client.fetch_optional_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_unique_select_query::<sqlx::Sqlite>(
                            &base_sql, &self.r#where,
                        );
                        self.client.fetch_optional_sqlite(qb).await
                    }
                }
            }
        }

        // ── FindFirstSelectQuery ─────────────────────────────────

        pub struct FindFirstSelectQuery<'a> {
            client: &'a DatabaseClient,
            r#where: filter::#_where_input,
            order_by: Vec<order::#order_by_name>,
            select: #select_name,
        }

        impl<'a> FindFirstSelectQuery<'a> {
            pub fn order_by(mut self, order: order::#order_by_name) -> Self {
                self.order_by.push(order);
                self
            }

            pub async fn exec(self) -> Result<Option<#partial_name>, FerriormError> {
                let cols = build_select_columns(&self.select);
                let base_sql = format!(#select_sql_prefix, cols);

                match self.client {
                    DatabaseClient::Postgres(_) => {
                        let qb = build_select_query::<sqlx::Postgres>(
                            &base_sql, &self.r#where, &self.order_by, Some(1), None,
                        );
                        self.client.fetch_optional_pg(qb).await
                    }
                    DatabaseClient::Sqlite(_) => {
                        let qb = build_select_query::<sqlx::Sqlite>(
                            &base_sql, &self.r#where, &self.order_by, Some(1), None,
                        );
                        self.client.fetch_optional_sqlite(qb).await
                    }
                }
            }
        }
    }
}
