use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum ParseError {
    #[error("Syntax error: {0}")]
    #[diagnostic(code(ormx::parser::syntax))]
    Syntax(String),

    #[error("Validation error: {0}")]
    #[diagnostic(code(ormx::parser::validation))]
    Validation(String),
}
