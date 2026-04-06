pub mod error;
pub mod parser;
pub mod validator;

pub use parser::parse;
pub use validator::validate;

/// Parse and validate a schema file in one step.
pub fn parse_and_validate(
    source: &str,
) -> Result<ormx_core::schema::Schema, error::ParseError> {
    let ast = parse(source)?;
    validate(&ast).map_err(|e| error::ParseError::Validation(e.to_string()))
}
