use ormx_core::ast::*;
use pest::Parser;
use pest_derive::Parser;

use crate::error::ParseError;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct OrmxParser;

/// Parse a `.ormx` schema string into an AST.
pub fn parse(source: &str) -> Result<SchemaFile, ParseError> {
    let pairs = OrmxParser::parse(Rule::schema, source)
        .map_err(|e| ParseError::Syntax(e.to_string()))?;

    let mut schema = SchemaFile {
        datasource: None,
        generators: Vec::new(),
        enums: Vec::new(),
        models: Vec::new(),
    };

    // The top-level parse result contains a single `schema` pair; iterate its inner pairs.
    let schema_pair = pairs.into_iter().next().unwrap();
    for pair in schema_pair.into_inner() {
        match pair.as_rule() {
            Rule::datasource_block => {
                schema.datasource = Some(parse_datasource(pair)?);
            }
            Rule::generator_block => {
                schema.generators.push(parse_generator(pair)?);
            }
            Rule::enum_block => {
                schema.enums.push(parse_enum(pair)?);
            }
            Rule::model_block => {
                schema.models.push(parse_model(pair)?);
            }
            Rule::EOI => {}
            _ => {}
        }
    }

    Ok(schema)
}

fn span_from(pair: &pest::iterators::Pair<'_, Rule>) -> Span {
    let span = pair.as_span();
    Span {
        start: span.start(),
        end: span.end(),
    }
}

fn parse_datasource(pair: pest::iterators::Pair<'_, Rule>) -> Result<Datasource, ParseError> {
    let span = span_from(&pair);
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();

    let mut provider = String::new();
    let mut url = StringOrEnv::Literal(String::new());

    for kv in inner {
        if kv.as_rule() != Rule::kv_pair {
            continue;
        }
        let mut kv_inner = kv.into_inner();
        let key = kv_inner.next().unwrap().as_str();
        let value_pair = kv_inner.next().unwrap();

        match key {
            "provider" => {
                provider = parse_string_value(&value_pair);
            }
            "url" => {
                url = parse_string_or_env(&value_pair)?;
            }
            _ => {}
        }
    }

    Ok(Datasource {
        name,
        provider,
        url,
        span,
    })
}

fn parse_generator(pair: pest::iterators::Pair<'_, Rule>) -> Result<Generator, ParseError> {
    let span = span_from(&pair);
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();

    let mut output = None;

    for kv in inner {
        if kv.as_rule() != Rule::kv_pair {
            continue;
        }
        let mut kv_inner = kv.into_inner();
        let key = kv_inner.next().unwrap().as_str();
        let value_pair = kv_inner.next().unwrap();

        match key {
            "output" => {
                output = Some(parse_string_value(&value_pair));
            }
            _ => {}
        }
    }

    Ok(Generator { name, output, span })
}

fn parse_enum(pair: pest::iterators::Pair<'_, Rule>) -> Result<EnumDef, ParseError> {
    let span = span_from(&pair);
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();

    let mut variants = Vec::new();
    for variant_pair in inner {
        if variant_pair.as_rule() == Rule::enum_variant {
            let variant_name = variant_pair.into_inner().next().unwrap().as_str().to_string();
            variants.push(variant_name);
        }
    }

    Ok(EnumDef {
        name,
        variants,
        db_name: None,
        span,
    })
}

fn parse_model(pair: pest::iterators::Pair<'_, Rule>) -> Result<ModelDef, ParseError> {
    let span = span_from(&pair);
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();

    let mut fields = Vec::new();
    let mut attributes = Vec::new();

    for member in inner {
        match member.as_rule() {
            Rule::field_def => {
                fields.push(parse_field(member)?);
            }
            Rule::block_attr_index => {
                attributes.push(BlockAttribute::Index(parse_field_list_from_block_attr(
                    member,
                )));
            }
            Rule::block_attr_unique => {
                attributes.push(BlockAttribute::Unique(parse_field_list_from_block_attr(
                    member,
                )));
            }
            Rule::block_attr_map => {
                let s = member
                    .into_inner()
                    .next()
                    .unwrap()
                    .as_str();
                attributes.push(BlockAttribute::Map(unquote(s)));
            }
            Rule::block_attr_id => {
                attributes.push(BlockAttribute::Id(parse_field_list_from_block_attr(
                    member,
                )));
            }
            _ => {}
        }
    }

    Ok(ModelDef {
        name,
        fields,
        attributes,
        span,
    })
}

fn parse_field(pair: pest::iterators::Pair<'_, Rule>) -> Result<FieldDef, ParseError> {
    let span = span_from(&pair);
    let mut inner = pair.into_inner();

    let name = inner.next().unwrap().as_str().to_string();
    let field_type_pair = inner.next().unwrap();
    let field_type = parse_field_type(field_type_pair);

    let mut attributes = Vec::new();
    for attr_pair in inner {
        if let Some(attr) = parse_field_attribute(attr_pair)? {
            attributes.push(attr);
        }
    }

    Ok(FieldDef {
        name,
        field_type,
        attributes,
        span,
    })
}

fn parse_field_type(pair: pest::iterators::Pair<'_, Rule>) -> FieldType {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();

    let mut is_list = false;
    let mut is_optional = false;

    for modifier in inner {
        match modifier.as_rule() {
            Rule::list_modifier => is_list = true,
            Rule::optional_modifier => is_optional = true,
            _ => {}
        }
    }

    FieldType {
        name,
        is_list,
        is_optional,
    }
}

fn parse_field_attribute(
    pair: pest::iterators::Pair<'_, Rule>,
) -> Result<Option<FieldAttribute>, ParseError> {
    match pair.as_rule() {
        Rule::attr_id => Ok(Some(FieldAttribute::Id)),
        Rule::attr_unique => Ok(Some(FieldAttribute::Unique)),
        Rule::attr_updated_at => Ok(Some(FieldAttribute::UpdatedAt)),
        Rule::attr_default => {
            let value_pair = pair.into_inner().next().unwrap();
            let default = parse_default_value(value_pair)?;
            Ok(Some(FieldAttribute::Default(default)))
        }
        Rule::attr_map => {
            let s = pair.into_inner().next().unwrap().as_str();
            Ok(Some(FieldAttribute::Map(unquote(s))))
        }
        Rule::attr_relation => {
            let relation = parse_relation_attribute(pair)?;
            Ok(Some(FieldAttribute::Relation(relation)))
        }
        Rule::attr_db_type => {
            let mut inner = pair.into_inner();
            let type_name = inner.next().unwrap().as_str().to_string();
            let args: Vec<String> = inner.map(|p| parse_string_value(&p)).collect();
            Ok(Some(FieldAttribute::DbType(type_name, args)))
        }
        _ => Ok(None),
    }
}

fn parse_default_value(
    pair: pest::iterators::Pair<'_, Rule>,
) -> Result<DefaultValue, ParseError> {
    match pair.as_rule() {
        Rule::func_call => {
            let mut inner = pair.into_inner();
            let func_name = inner.next().unwrap().as_str();
            match func_name {
                "uuid" => Ok(DefaultValue::Uuid),
                "cuid" => Ok(DefaultValue::Cuid),
                "autoincrement" => Ok(DefaultValue::AutoIncrement),
                "now" => Ok(DefaultValue::Now),
                other => Err(ParseError::Syntax(format!(
                    "Unknown default function: {other}()"
                ))),
            }
        }
        Rule::string_literal => {
            Ok(DefaultValue::Literal(LiteralValue::String(unquote(pair.as_str()))))
        }
        Rule::number_literal => {
            let s = pair.as_str();
            if s.contains('.') {
                Ok(DefaultValue::Literal(LiteralValue::Float(
                    s.parse().map_err(|e| ParseError::Syntax(format!("Invalid float: {e}")))?,
                )))
            } else {
                Ok(DefaultValue::Literal(LiteralValue::Int(
                    s.parse().map_err(|e| ParseError::Syntax(format!("Invalid int: {e}")))?,
                )))
            }
        }
        Rule::boolean_literal => {
            let b = pair.as_str() == "true";
            Ok(DefaultValue::Literal(LiteralValue::Bool(b)))
        }
        Rule::identifier_value => {
            let name = pair.into_inner().next().unwrap().as_str().to_string();
            Ok(DefaultValue::EnumVariant(name))
        }
        _ => Err(ParseError::Syntax(format!(
            "Unexpected default value: {:?}",
            pair.as_rule()
        ))),
    }
}

fn parse_relation_attribute(
    pair: pest::iterators::Pair<'_, Rule>,
) -> Result<RelationAttribute, ParseError> {
    let args_pair = pair.into_inner().next().unwrap(); // relation_args
    let mut fields = Vec::new();
    let mut references = Vec::new();
    let mut on_delete = None;
    let mut on_update = None;
    let mut name = None;

    for arg in args_pair.into_inner() {
        // relation_arg = { named_arg }
        // Each arg is a relation_arg containing a named_arg
        let named_arg = if arg.as_rule() == Rule::relation_arg {
            arg.into_inner().next().unwrap()
        } else if arg.as_rule() == Rule::named_arg {
            arg
        } else {
            continue;
        };

        // named_arg = { identifier ~ ":" ~ (field_list | value) }
        let mut named = named_arg.into_inner();
        let key = named.next().unwrap().as_str();
        let value_pair = named.next().unwrap();

        match key {
            "fields" => fields = parse_field_list(&value_pair),
            "references" => references = parse_field_list(&value_pair),
            "onDelete" => on_delete = parse_referential_action(&value_pair),
            "onUpdate" => on_update = parse_referential_action(&value_pair),
            "name" => name = Some(parse_string_value(&value_pair)),
            _ => {}
        }
    }

    Ok(RelationAttribute {
        name,
        fields,
        references,
        on_delete,
        on_update,
    })
}

fn parse_referential_action(pair: &pest::iterators::Pair<'_, Rule>) -> Option<ReferentialAction> {
    let s = pair.as_str().trim_matches('"');
    match s {
        "Cascade" => Some(ReferentialAction::Cascade),
        "Restrict" => Some(ReferentialAction::Restrict),
        "NoAction" => Some(ReferentialAction::NoAction),
        "SetNull" => Some(ReferentialAction::SetNull),
        "SetDefault" => Some(ReferentialAction::SetDefault),
        _ => None,
    }
}

fn parse_field_list(pair: &pest::iterators::Pair<'_, Rule>) -> Vec<String> {
    pair.clone()
        .into_inner()
        .filter(|p| p.as_rule() == Rule::identifier)
        .map(|p| p.as_str().to_string())
        .collect()
}

fn parse_field_list_from_block_attr(pair: pest::iterators::Pair<'_, Rule>) -> Vec<String> {
    let field_list = pair.into_inner().next().unwrap();
    parse_field_list(&field_list)
}

fn parse_string_or_env(
    pair: &pest::iterators::Pair<'_, Rule>,
) -> Result<StringOrEnv, ParseError> {
    match pair.as_rule() {
        Rule::func_call => {
            let mut inner = pair.clone().into_inner();
            let func_name = inner.next().unwrap().as_str();
            if func_name == "env" {
                let arg = inner.next().ok_or_else(|| {
                    ParseError::Syntax("env() requires a string argument".into())
                })?;
                Ok(StringOrEnv::Env(unquote(arg.as_str())))
            } else {
                Err(ParseError::Syntax(format!(
                    "Expected env(), got {func_name}()"
                )))
            }
        }
        Rule::string_literal => Ok(StringOrEnv::Literal(unquote(pair.as_str()))),
        _ => Err(ParseError::Syntax(format!(
            "Expected string or env(), got {:?}",
            pair.as_rule()
        ))),
    }
}

fn parse_string_value(pair: &pest::iterators::Pair<'_, Rule>) -> String {
    unquote(pair.as_str())
}

fn unquote(s: &str) -> String {
    if s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

generator client {
  output = "./src/generated"
}

enum Role {
  User
  Admin
  Moderator
}

model User {
  id        String   @id @default(uuid())
  email     String   @unique
  name      String?
  role      Role     @default(User)
  createdAt DateTime @default(now())
  updatedAt DateTime @updatedAt

  @@index([email])
  @@map("users")
}
"#;

    #[test]
    fn test_parse_basic_schema() {
        let schema = parse(BASIC_SCHEMA).expect("should parse");

        // Datasource
        let ds = schema.datasource.expect("should have datasource");
        assert_eq!(ds.name, "db");
        assert_eq!(ds.provider, "postgresql");
        match &ds.url {
            StringOrEnv::Env(var) => assert_eq!(var, "DATABASE_URL"),
            _ => panic!("expected env()"),
        }

        // Generator
        assert_eq!(schema.generators.len(), 1);
        assert_eq!(schema.generators[0].name, "client");
        assert_eq!(
            schema.generators[0].output.as_deref(),
            Some("./src/generated")
        );

        // Enum
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].name, "Role");
        assert_eq!(
            schema.enums[0].variants,
            vec!["User", "Admin", "Moderator"]
        );

        // Model
        assert_eq!(schema.models.len(), 1);
        let user = &schema.models[0];
        assert_eq!(user.name, "User");
        assert_eq!(user.fields.len(), 6);

        // id field
        let id_field = &user.fields[0];
        assert_eq!(id_field.name, "id");
        assert_eq!(id_field.field_type.name, "String");
        assert!(!id_field.field_type.is_optional);
        assert!(id_field
            .attributes
            .iter()
            .any(|a| matches!(a, FieldAttribute::Id)));
        assert!(id_field
            .attributes
            .iter()
            .any(|a| matches!(a, FieldAttribute::Default(DefaultValue::Uuid))));

        // name field is optional
        let name_field = &user.fields[2];
        assert_eq!(name_field.name, "name");
        assert!(name_field.field_type.is_optional);

        // role field has enum default
        let role_field = &user.fields[3];
        assert_eq!(role_field.name, "role");
        assert!(role_field.attributes.iter().any(
            |a| matches!(a, FieldAttribute::Default(DefaultValue::EnumVariant(v)) if v == "User")
        ));

        // updatedAt has @updatedAt
        let updated_field = &user.fields[5];
        assert_eq!(updated_field.name, "updatedAt");
        assert!(updated_field
            .attributes
            .iter()
            .any(|a| matches!(a, FieldAttribute::UpdatedAt)));

        // Block attributes
        assert_eq!(user.attributes.len(), 2);
        assert!(user
            .attributes
            .iter()
            .any(|a| matches!(a, BlockAttribute::Index(fields) if fields == &["email"])));
        assert!(user
            .attributes
            .iter()
            .any(|a| matches!(a, BlockAttribute::Map(name) if name == "users")));
    }

    #[test]
    fn test_parse_multiple_models() {
        let schema_str = r#"
datasource db {
  provider = "postgresql"
  url      = "postgres://localhost/test"
}

model User {
  id    String @id @default(uuid())
  email String @unique
  posts Post[]
}

model Post {
  id       String  @id @default(uuid())
  title    String
  content  String?
  author   User    @relation(fields: [authorId], references: [id])
  authorId String

  @@index([authorId])
}
"#;

        let schema = parse(schema_str).expect("should parse");
        assert_eq!(schema.models.len(), 2);
        assert_eq!(schema.models[0].name, "User");
        assert_eq!(schema.models[1].name, "Post");

        // Check relation attribute on Post.author
        let author_field = &schema.models[1].fields[3];
        assert_eq!(author_field.name, "author");
        let rel = author_field.attributes.iter().find_map(|a| match a {
            FieldAttribute::Relation(r) => Some(r),
            _ => None,
        });
        let rel = rel.expect("should have @relation");
        assert_eq!(rel.fields, vec!["authorId"]);
        assert_eq!(rel.references, vec!["id"]);

        // Check Post[] is a list
        let posts_field = &schema.models[0].fields[2];
        assert_eq!(posts_field.name, "posts");
        assert!(posts_field.field_type.is_list);
    }

    #[test]
    fn test_parse_composite_id() {
        let schema_str = r#"
datasource db {
  provider = "sqlite"
  url      = "file:./dev.db"
}

model PostTag {
  postId String
  tagId  String

  @@id([postId, tagId])
}
"#;

        let schema = parse(schema_str).expect("should parse");
        let model = &schema.models[0];
        assert!(model
            .attributes
            .iter()
            .any(|a| matches!(a, BlockAttribute::Id(fields) if fields == &["postId", "tagId"])));
    }

    #[test]
    fn test_parse_error_invalid_syntax() {
        let bad = "model { broken }";
        assert!(parse(bad).is_err());
    }

    #[test]
    fn test_parse_with_comments() {
        let schema_str = r#"
// This is a comment
datasource db {
  provider = "postgresql" // inline comment
  url      = env("DATABASE_URL")
}

// Another comment
model User {
  id String @id @default(uuid())
  // A commented field
  name String?
}
"#;

        let schema = parse(schema_str).expect("should parse with comments");
        assert!(schema.datasource.is_some());
        assert_eq!(schema.models[0].fields.len(), 2);
    }
}
