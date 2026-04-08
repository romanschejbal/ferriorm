//! Parameterized SQL query builder.
//!
//! [`SqlBuilder`] constructs SQL strings with properly indexed parameter
//! placeholders. It supports both `PostgreSQL` (`$1`, `$2`, ...) and `SQLite`
//! (`?`) placeholder styles, selected automatically based on the active
//! [`DatabaseClient`]. The builder also provides safe identifier quoting to
//! prevent SQL injection in table and column names.

use crate::client::DatabaseClient;

/// Determines the SQL placeholder style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamStyle {
    /// `PostgreSQL`: `$1`, `$2`, etc.
    Dollar,
    /// SQLite/MySQL: `?`
    QuestionMark,
}

impl ParamStyle {
    /// Determine the placeholder style from a database client.
    #[must_use]
    pub fn from_client(client: &DatabaseClient) -> Self {
        match client {
            #[cfg(feature = "postgres")]
            DatabaseClient::Postgres(_) => Self::Dollar,
            #[cfg(feature = "sqlite")]
            DatabaseClient::Sqlite(_) => Self::QuestionMark,
        }
    }
}

/// A safe parameterized SQL builder.
///
/// Tracks parameter bindings and builds SQL strings with placeholders.
/// Supports both `PostgreSQL` (`$1`) and `SQLite` (`?`) styles.
///
/// Note: the generated client code uses `sqlx::QueryBuilder` directly for its
/// queries. This builder is available for advanced use cases where manual SQL
/// construction with database-aware placeholders is needed.
#[derive(Debug)]
pub struct SqlBuilder {
    sql: String,
    param_count: usize,
    style: ParamStyle,
}

impl SqlBuilder {
    /// Create a new SQL builder with the given placeholder style.
    #[must_use]
    pub fn new(style: ParamStyle) -> Self {
        Self {
            sql: String::with_capacity(256),
            param_count: 0,
            style,
        }
    }

    /// Create a new SQL builder with the placeholder style matching the client.
    #[must_use]
    pub fn for_client(client: &DatabaseClient) -> Self {
        Self::new(ParamStyle::from_client(client))
    }

    /// Append raw SQL text.
    pub fn push(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    /// Append a single character.
    pub fn push_char(&mut self, c: char) {
        self.sql.push(c);
    }

    /// Append a parameter placeholder and increment the counter.
    /// Returns the parameter index (1-based).
    pub fn push_param(&mut self) -> usize {
        self.param_count += 1;
        match self.style {
            ParamStyle::Dollar => {
                self.sql.push('$');
                self.sql.push_str(&self.param_count.to_string());
            }
            ParamStyle::QuestionMark => {
                self.sql.push('?');
            }
        }
        self.param_count
    }

    /// Append a quoted identifier (table or column name).
    pub fn push_identifier(&mut self, name: &str) {
        self.sql.push('"');
        // Escape any double quotes in the name
        for c in name.chars() {
            if c == '"' {
                self.sql.push('"');
            }
            self.sql.push(c);
        }
        self.sql.push('"');
    }

    /// Get the current parameter count.
    #[must_use]
    pub fn param_count(&self) -> usize {
        self.param_count
    }

    /// Get the current parameter style.
    #[must_use]
    pub fn style(&self) -> ParamStyle {
        self.style
    }

    /// Consume the builder and return the SQL string.
    #[must_use]
    pub fn build(self) -> String {
        self.sql
    }

    /// Get the SQL string by reference.
    #[must_use]
    pub fn sql(&self) -> &str {
        &self.sql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_params() {
        let mut b = SqlBuilder::new(ParamStyle::Dollar);
        b.push("SELECT * FROM ");
        b.push_identifier("users");
        b.push(" WHERE ");
        b.push_identifier("email");
        b.push(" = ");
        b.push_param();
        b.push(" AND ");
        b.push_identifier("age");
        b.push(" > ");
        b.push_param();

        assert_eq!(
            b.build(),
            r#"SELECT * FROM "users" WHERE "email" = $1 AND "age" > $2"#
        );
    }

    #[test]
    fn test_sqlite_params() {
        let mut b = SqlBuilder::new(ParamStyle::QuestionMark);
        b.push("SELECT * FROM ");
        b.push_identifier("users");
        b.push(" WHERE ");
        b.push_identifier("email");
        b.push(" = ");
        b.push_param();

        assert_eq!(b.build(), r#"SELECT * FROM "users" WHERE "email" = ?"#);
    }
}
