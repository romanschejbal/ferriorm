//! Code generation for relation support: Include, WithRelations, batched loading.

use ferriorm_core::schema::{Field, FieldKind, Model, RelationType, Schema};
use ferriorm_core::utils::to_snake_case;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Information about a relation from one model to another.
pub struct RelationInfo<'a> {
    pub field: &'a Field,
    pub related_model: &'a Model,
    pub relation_type: RelationType,
    /// The FK column on the "many" side (e.g., "author_id" on Post for User.posts)
    pub fk_column: String,
    /// The referenced column (e.g., "id" on User)
    pub ref_column: String,
}

/// Collect relation fields for a model, resolving against the full schema.
pub fn collect_relations<'a>(model: &'a Model, schema: &'a Schema) -> Vec<RelationInfo<'a>> {
    let mut relations = Vec::new();

    for field in &model.fields {
        if let Some(rel) = &field.relation {
            let related = schema.models.iter().find(|m| m.name == rel.related_model);
            if let Some(related_model) = related {
                let (fk_column, ref_column) = if !rel.fields.is_empty() {
                    // This side has the FK (ManyToOne)
                    (rel.fields[0].clone(), rel.references[0].clone())
                } else {
                    // The other side has the FK (OneToMany) — find the back-reference
                    find_back_reference(model, related_model)
                        .unwrap_or_else(|| ("id".into(), "id".into()))
                };

                relations.push(RelationInfo {
                    field,
                    related_model,
                    relation_type: rel.relation_type,
                    fk_column: to_snake_case(&fk_column),
                    ref_column: to_snake_case(&ref_column),
                });
            }
        } else if field.is_list {
            // Implicit relation (e.g., posts Post[])
            if let FieldKind::Model(related_name) = &field.field_type {
                let related = schema.models.iter().find(|m| m.name == *related_name);
                if let Some(related_model) = related {
                    let (fk_column, ref_column) = find_back_reference(model, related_model)
                        .unwrap_or_else(|| ("id".into(), "id".into()));

                    relations.push(RelationInfo {
                        field,
                        related_model,
                        relation_type: RelationType::OneToMany,
                        fk_column: to_snake_case(&fk_column),
                        ref_column: to_snake_case(&ref_column),
                    });
                }
            }
        }
    }

    relations
}

/// Find the back-reference from the related model to this model.
/// E.g., for User.posts (Post[]), find Post.authorId @relation(fields: [authorId], references: [id])
fn find_back_reference(parent: &Model, child: &Model) -> Option<(String, String)> {
    for field in &child.fields {
        if let Some(rel) = &field.relation
            && rel.related_model == parent.name
            && !rel.fields.is_empty()
        {
            return Some((rel.fields[0].clone(), rel.references[0].clone()));
        }
    }
    None
}

/// Generate the Include struct, WithRelations struct, and include-aware query methods.
pub fn gen_relation_types(model: &Model, schema: &Schema) -> TokenStream {
    let relations = collect_relations(model, schema);

    if relations.is_empty() {
        return quote! {};
    }

    let model_ident = format_ident!("{}", model.name);
    let include_name = format_ident!("{}Include", model.name);
    let with_relations_name = format_ident!("{}WithRelations", model.name);

    // Include struct fields
    let include_fields: Vec<TokenStream> = relations
        .iter()
        .map(|r| {
            let name = format_ident!("{}", to_snake_case(&r.field.name));
            quote! { pub #name: bool }
        })
        .collect();

    // WithRelations struct fields
    let with_rel_fields: Vec<TokenStream> = relations
        .iter()
        .map(|r| {
            let name = format_ident!("{}", to_snake_case(&r.field.name));
            let related_mod = format_ident!("{}", to_snake_case(&r.related_model.name));
            let related_struct = format_ident!("{}", r.related_model.name);

            match r.relation_type {
                RelationType::OneToMany | RelationType::ManyToMany => {
                    quote! { pub #name: Option<Vec<super::#related_mod::#related_struct>> }
                }
                RelationType::OneToOne | RelationType::ManyToOne => {
                    quote! { pub #name: Option<super::#related_mod::#related_struct> }
                }
            }
        })
        .collect();

    // Generate the batched loading logic
    let load_arms = gen_load_arms(&relations, model);

    quote! {
        #[derive(Debug, Clone, Default)]
        pub struct #include_name {
            #(#include_fields,)*
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct #with_relations_name {
            #[serde(flatten)]
            pub data: #model_ident,
            #(#with_rel_fields,)*
        }

        impl #model_ident {
            /// Load relations for a batch of records.
            pub(crate) async fn load_relations(
                records: Vec<#model_ident>,
                include: &#include_name,
                client: &DatabaseClient,
            ) -> Result<Vec<#with_relations_name>, FerriormError> {
                #load_arms
            }
        }
    }
}

/// Helper: generate code to load related rows using QueryBuilder and dispatch
/// to both Postgres and Sqlite.
fn gen_batched_load_many(
    rel: &RelationInfo<'_>,
    load_var: &proc_macro2::Ident,
    field_name: &proc_macro2::Ident,
    id_source_ident: &proc_macro2::Ident,
    lookup_col_str: &str,
    insert_key_ident: &proc_macro2::Ident,
    fk_optional: bool,
) -> TokenStream {
    let related_mod = format_ident!("{}", to_snake_case(&rel.related_model.name));
    let related_struct = format_ident!("{}", rel.related_model.name);
    let related_table = &rel.related_model.db_name;

    let select_base = format!(
        r#"SELECT * FROM "{}" WHERE "{}" IN ("#,
        related_table, lookup_col_str
    );

    // Generate the row insertion code based on whether the FK is optional
    let insert_row_code = if fk_optional {
        quote! {
            if let Some(key) = row.#insert_key_ident.clone() {
                #load_var.entry(key).or_default().push(row);
            }
        }
    } else {
        quote! {
            #load_var.entry(row.#insert_key_ident.clone()).or_default().push(row);
        }
    };

    quote! {
        let mut #load_var: std::collections::HashMap<String, Vec<super::#related_mod::#related_struct>> = std::collections::HashMap::new();
        if include.#field_name {
            let ids: Vec<String> = records.iter()
                .map(|r| r.#id_source_ident.clone())
                .collect();

            if !ids.is_empty() {
                macro_rules! build_in_query {
                    ($db:ty) => {{
                        let mut qb = sqlx::QueryBuilder::<$db>::new(#select_base);
                        let mut sep = qb.separated(", ");
                        for id in &ids {
                            sep.push_bind(id.clone());
                        }
                        qb.push(")");
                        qb
                    }};
                }

                macro_rules! insert_rows {
                    ($rows:expr) => {
                        for row in $rows {
                            #insert_row_code
                        }
                    };
                }

                match client {
                    DatabaseClient::Postgres(pool) => {
                        let mut qb = build_in_query!(sqlx::Postgres);
                        let related_rows: Vec<super::#related_mod::#related_struct> =
                            qb.build_query_as().fetch_all(pool).await
                                .map_err(FerriormError::from)?;
                        insert_rows!(related_rows);
                    }
                    DatabaseClient::Sqlite(pool) => {
                        let mut qb = build_in_query!(sqlx::Sqlite);
                        let related_rows: Vec<super::#related_mod::#related_struct> =
                            qb.build_query_as().fetch_all(pool).await
                                .map_err(FerriormError::from)?;
                        insert_rows!(related_rows);
                    }
                }
            }
        }
    }
}

/// Helper: generate code to load related rows for a single-value (OneToOne / ManyToOne) relation.
fn gen_batched_load_one(
    rel: &RelationInfo<'_>,
    load_var: &proc_macro2::Ident,
    field_name: &proc_macro2::Ident,
    id_source_ident: &proc_macro2::Ident,
    lookup_col_str: &str,
    insert_key_ident: &proc_macro2::Ident,
    fk_is_optional: bool,
) -> TokenStream {
    let related_mod = format_ident!("{}", to_snake_case(&rel.related_model.name));
    let related_struct = format_ident!("{}", rel.related_model.name);
    let related_table = &rel.related_model.db_name;

    let select_base = format!(
        r#"SELECT * FROM "{}" WHERE "{}" IN ("#,
        related_table, lookup_col_str
    );

    // When the FK field is optional (Option<String>), use filter_map to skip None values.
    let ids_collect = if fk_is_optional {
        quote! {
            let ids: Vec<String> = records.iter()
                .filter_map(|r| r.#id_source_ident.clone())
                .collect();
        }
    } else {
        quote! {
            let ids: Vec<String> = records.iter()
                .map(|r| r.#id_source_ident.clone())
                .collect();
        }
    };

    quote! {
        let mut #load_var: std::collections::HashMap<String, super::#related_mod::#related_struct> = std::collections::HashMap::new();
        if include.#field_name {
            #ids_collect

            if !ids.is_empty() {
                macro_rules! build_in_query {
                    ($db:ty) => {{
                        let mut qb = sqlx::QueryBuilder::<$db>::new(#select_base);
                        let mut sep = qb.separated(", ");
                        for id in &ids {
                            sep.push_bind(id.clone());
                        }
                        qb.push(")");
                        qb
                    }};
                }

                match client {
                    DatabaseClient::Postgres(pool) => {
                        let mut qb = build_in_query!(sqlx::Postgres);
                        let related_rows: Vec<super::#related_mod::#related_struct> =
                            qb.build_query_as().fetch_all(pool).await
                                .map_err(FerriormError::from)?;
                        for row in related_rows {
                            #load_var.insert(row.#insert_key_ident.clone(), row);
                        }
                    }
                    DatabaseClient::Sqlite(pool) => {
                        let mut qb = build_in_query!(sqlx::Sqlite);
                        let related_rows: Vec<super::#related_mod::#related_struct> =
                            qb.build_query_as().fetch_all(pool).await
                                .map_err(FerriormError::from)?;
                        for row in related_rows {
                            #load_var.insert(row.#insert_key_ident.clone(), row);
                        }
                    }
                }
            }
        }
    }
}

fn gen_load_arms(relations: &[RelationInfo<'_>], model: &Model) -> TokenStream {
    let _model_ident = format_ident!("{}", model.name);
    let with_relations_name = format_ident!("{}WithRelations", model.name);

    let mut relation_loads = vec![];
    let mut field_inits = vec![];

    for rel in relations {
        let field_name = format_ident!("{}", to_snake_case(&rel.field.name));
        let fk_col_str = &rel.fk_column;
        let ref_col_str = &rel.ref_column;
        let fk_col_ident = format_ident!("{}", rel.fk_column);
        let ref_col_ident = format_ident!("{}", rel.ref_column);

        match rel.relation_type {
            RelationType::OneToMany | RelationType::ManyToMany => {
                // Batched loading: SELECT * FROM related WHERE fk IN (parent_ids)
                let load_var = format_ident!("{}_map", to_snake_case(&rel.field.name));

                // Check if the FK column on the child (related) model is optional
                let child_fk_optional = rel
                    .related_model
                    .fields
                    .iter()
                    .any(|f| to_snake_case(&f.name) == *fk_col_str && f.is_optional);

                relation_loads.push(gen_batched_load_many(
                    rel,
                    &load_var,
                    &field_name,
                    &ref_col_ident,
                    fk_col_str,
                    &fk_col_ident,
                    child_fk_optional,
                ));

                let ref_col_ident = format_ident!("{}", ref_col_str);
                field_inits.push(quote! {
                    #field_name: if include.#field_name {
                        Some(#load_var.remove(&r.#ref_col_ident).unwrap_or_default())
                    } else {
                        None
                    }
                });
            }
            RelationType::OneToOne | RelationType::ManyToOne => {
                // For ManyToOne (this model has the FK), we can batch load the parent
                let load_var = format_ident!("{}_map", to_snake_case(&rel.field.name));
                let fk_field = format_ident!("{}", fk_col_str);

                // Check if this model has the FK field as a scalar
                let fk_model_field = model
                    .fields
                    .iter()
                    .find(|f| to_snake_case(&f.name) == *fk_col_str && f.is_scalar());
                let has_fk = fk_model_field.is_some();
                let fk_is_optional = fk_model_field.map(|f| f.is_optional).unwrap_or(false);

                if has_fk {
                    relation_loads.push(gen_batched_load_one(
                        rel,
                        &load_var,
                        &field_name,
                        &fk_field,
                        ref_col_str,
                        &ref_col_ident,
                        fk_is_optional,
                    ));

                    if fk_is_optional {
                        field_inits.push(quote! {
                            #field_name: if include.#field_name {
                                r.#fk_field.as_ref().and_then(|fk| #load_var.remove(fk))
                            } else {
                                None
                            }
                        });
                    } else {
                        field_inits.push(quote! {
                            #field_name: if include.#field_name {
                                #load_var.remove(&r.#fk_field).map(Some).unwrap_or(None)
                            } else {
                                None
                            }
                        });
                    }
                } else {
                    // The FK is on the other side (e.g., User.profile where Profile has userId)
                    // Batch load: SELECT * FROM profiles WHERE user_id IN (user_ids)
                    let ref_col_ident = format_ident!("{}", ref_col_str);

                    relation_loads.push(gen_batched_load_one(
                        rel,
                        &load_var,
                        &field_name,
                        &ref_col_ident,
                        fk_col_str,
                        &fk_col_ident,
                        false,
                    ));

                    field_inits.push(quote! {
                        #field_name: if include.#field_name {
                            #load_var.remove(&r.#ref_col_ident)
                        } else {
                            None
                        }
                    });
                }
            }
        }
    }

    quote! {
        #(#relation_loads)*

        let mut results = Vec::with_capacity(records.len());
        for r in records {
            results.push(#with_relations_name {
                #(#field_inits,)*
                data: r,
            });
        }
        Ok(results)
    }
}

/// Generate `include()` and `exec_with_relations()` methods for FindMany.
pub fn gen_find_many_include(model: &Model, schema: &Schema) -> TokenStream {
    let relations = collect_relations(model, schema);
    if relations.is_empty() {
        return quote! {};
    }

    let model_ident = format_ident!("{}", model.name);
    let include_name = format_ident!("{}Include", model.name);
    let with_relations_name = format_ident!("{}WithRelations", model.name);

    quote! {
        impl<'a> FindManyQuery<'a> {
            pub fn include(self, include: #include_name) -> FindManyWithIncludeQuery<'a> {
                FindManyWithIncludeQuery {
                    inner: self,
                    include,
                }
            }
        }

        pub struct FindManyWithIncludeQuery<'a> {
            inner: FindManyQuery<'a>,
            include: #include_name,
        }

        impl<'a> FindManyWithIncludeQuery<'a> {
            pub async fn exec(self) -> Result<Vec<#with_relations_name>, FerriormError> {
                let include = self.include;
                let client = self.inner.client;
                let records = FindManyQuery {
                    client,
                    r#where: self.inner.r#where,
                    order_by: self.inner.order_by,
                    skip: self.inner.skip,
                    take: self.inner.take,
                }.exec().await?;
                #model_ident::load_relations(records, &include, client).await
            }
        }

        impl<'a> FindUniqueQuery<'a> {
            pub fn include(self, include: #include_name) -> FindUniqueWithIncludeQuery<'a> {
                FindUniqueWithIncludeQuery {
                    inner: self,
                    include,
                }
            }
        }

        pub struct FindUniqueWithIncludeQuery<'a> {
            inner: FindUniqueQuery<'a>,
            include: #include_name,
        }

        impl<'a> FindUniqueWithIncludeQuery<'a> {
            pub async fn exec(self) -> Result<Option<#with_relations_name>, FerriormError> {
                let include = self.include;
                let client = self.inner.client;
                let record = FindUniqueQuery {
                    client,
                    r#where: self.inner.r#where,
                }.exec().await?;
                match record {
                    Some(r) => {
                        let mut results = #model_ident::load_relations(vec![r], &include, client).await?;
                        Ok(results.pop())
                    }
                    None => Ok(None),
                }
            }
        }
    }
}
