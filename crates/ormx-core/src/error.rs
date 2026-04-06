use core::fmt;

/// Core errors that can occur in ormx domain logic.
#[derive(Debug)]
pub enum CoreError {
    /// A model is missing a primary key (`@id` or `@@id`).
    MissingPrimaryKey {
        model_name: String,
    },

    /// A field references an unknown type.
    UnknownType {
        model_name: String,
        field_name: String,
        type_name: String,
    },

    /// A default value doesn't match the field type.
    InvalidDefault {
        model_name: String,
        field_name: String,
        message: String,
    },

    /// Duplicate model or enum name.
    DuplicateName {
        name: String,
        kind: &'static str,
    },

    /// A `@relation` attribute references unknown fields.
    InvalidRelationFields {
        model_name: String,
        field_name: String,
        message: String,
    },

    /// Unknown database provider.
    UnknownProvider {
        provider: String,
    },

    /// Generic validation error.
    Validation {
        message: String,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrimaryKey { model_name } => {
                write!(f, "Model `{model_name}` is missing a primary key (@id or @@id)")
            }
            Self::UnknownType { model_name, field_name, type_name } => {
                write!(f, "Unknown type `{type_name}` in field `{field_name}` of model `{model_name}`")
            }
            Self::InvalidDefault { model_name, field_name, message } => {
                write!(f, "Invalid default for `{model_name}.{field_name}`: {message}")
            }
            Self::DuplicateName { name, kind } => {
                write!(f, "Duplicate {kind} name: `{name}`")
            }
            Self::InvalidRelationFields { model_name, field_name, message } => {
                write!(f, "Invalid @relation on `{model_name}.{field_name}`: {message}")
            }
            Self::UnknownProvider { provider } => {
                write!(f, "Unknown database provider: `{provider}`")
            }
            Self::Validation { message } => {
                write!(f, "Validation error: {message}")
            }
        }
    }
}
