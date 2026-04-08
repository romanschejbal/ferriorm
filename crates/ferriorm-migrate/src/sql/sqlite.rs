//! `SQLite` SQL renderer for migration steps.
//!
//! Handles SQLite-specific limitations: enums are represented as TEXT columns
//! (with explanatory comments in the output), `ALTER COLUMN` is not supported
//! (a comment is emitted instead), and foreign key constraints cannot be added
//! after table creation. `SERIAL` types are mapped to `INTEGER`.

use super::SqlRenderer;
use crate::diff::{ColumnChanges, ColumnDef, CreateTable, ForeignKeyDef, MigrationStep};
use std::collections::HashMap;

pub struct SqliteRenderer;

impl SqlRenderer for SqliteRenderer {
    fn render(&self, steps: &[MigrationStep]) -> String {
        // First pass: collect all AddForeignKey steps keyed by table name.
        // These will be inlined into the corresponding CREATE TABLE statements
        // because SQLite does not support ALTER TABLE ADD CONSTRAINT FOREIGN KEY.
        let mut fk_map: HashMap<&str, Vec<&ForeignKeyDef>> = HashMap::new();
        for step in steps {
            if let MigrationStep::AddForeignKey(fk) = step {
                fk_map.entry(fk.table.as_str()).or_default().push(fk);
            }
        }

        let mut sql = String::new();
        sql.push_str("PRAGMA foreign_keys = ON;\n\n");
        for step in steps {
            match step {
                // Skip standalone AddForeignKey steps whose table has a
                // corresponding CreateTable — they are already inlined.
                MigrationStep::AddForeignKey(fk)
                    if steps.iter().any(
                        |s| matches!(s, MigrationStep::CreateTable(ct) if ct.name == fk.table),
                    ) =>
                {
                    // Already rendered inline in the CREATE TABLE.
                }
                MigrationStep::CreateTable(ct) => {
                    let fks = fk_map.get(ct.name.as_str()).map(std::vec::Vec::as_slice);
                    sql.push_str(&render_create_table(ct, fks));
                    sql.push('\n');
                }
                _ => {
                    sql.push_str(&render_step(step));
                    sql.push('\n');
                }
            }
        }
        sql
    }
}

fn render_step(step: &MigrationStep) -> String {
    match step {
        // SQLite has no CREATE TYPE — enums are stored as TEXT columns.
        // Emit a comment so the user knows this was intentionally skipped.
        MigrationStep::CreateEnum { name, variants } => {
            let vals = variants
                .iter()
                .map(|v| format!("'{}'", v.to_lowercase()))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "-- SQLite: enum \"{name}\" with values ({vals}) is represented as TEXT columns.\n"
            )
        }
        MigrationStep::DropEnum { name } => {
            format!(
                "-- SQLite: enum \"{name}\" does not exist as a separate type; nothing to drop.\n"
            )
        }
        MigrationStep::AddEnumVariant { enum_name, variant } => {
            format!(
                "-- SQLite: enum variant '{}' added to \"{enum_name}\" (no DDL needed, stored as TEXT).\n",
                variant.to_lowercase()
            )
        }
        MigrationStep::CreateTable(ct) => render_create_table(ct, None),
        MigrationStep::DropTable { name } => {
            format!("DROP TABLE IF EXISTS \"{name}\";\n")
        }
        MigrationStep::AddColumn { table, column } => {
            format!(
                "ALTER TABLE \"{}\" ADD COLUMN {};\n",
                table,
                render_column_def(column)
            )
        }
        MigrationStep::DropColumn { table, column } => {
            format!("ALTER TABLE \"{table}\" DROP COLUMN \"{column}\";\n")
        }
        MigrationStep::AlterColumn {
            table,
            column,
            changes,
        } => render_alter_column(table, column, changes),
        MigrationStep::CreateIndex {
            table,
            name,
            columns,
        } => {
            let cols = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("CREATE INDEX \"{name}\" ON \"{table}\" ({cols});\n")
        }
        MigrationStep::DropIndex { table: _, name } => {
            format!("DROP INDEX IF EXISTS \"{name}\";\n")
        }
        // SQLite does not support ADD CONSTRAINT for foreign keys after table creation.
        // Emit a comment explaining the limitation.
        MigrationStep::AddForeignKey(fk) => {
            format!(
                "-- SQLite: cannot add foreign key after table creation.\n\
                 -- Foreign key: \"{}\".\"{}\" -> \"{}\".\"{}\" ON DELETE {} ON UPDATE {}\n\
                 -- To add this constraint, recreate the table with the foreign key in CREATE TABLE.\n",
                fk.table,
                fk.column,
                fk.referenced_table,
                fk.referenced_column,
                fk.on_delete,
                fk.on_update
            )
        }
        MigrationStep::DropForeignKey { table, name } => {
            format!(
                "-- SQLite: cannot drop foreign key constraint \"{name}\" from \"{table}\" without recreating the table.\n"
            )
        }
        MigrationStep::AddUniqueConstraint {
            table,
            name,
            columns,
        } => {
            // SQLite supports CREATE UNIQUE INDEX as an alternative to ADD CONSTRAINT UNIQUE.
            let cols = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("CREATE UNIQUE INDEX \"{name}\" ON \"{table}\" ({cols});\n")
        }
        MigrationStep::DropUniqueConstraint { table: _, name } => {
            // Unique constraints created via CREATE UNIQUE INDEX can be dropped this way.
            format!("DROP INDEX IF EXISTS \"{name}\";\n")
        }
    }
}

fn render_create_table(ct: &CreateTable, fks: Option<&[&ForeignKeyDef]>) -> String {
    let mut sql = format!("CREATE TABLE \"{}\" (\n", ct.name);

    for (i, col) in ct.columns.iter().enumerate() {
        if i > 0 {
            sql.push_str(",\n");
        }
        sql.push_str("    ");
        sql.push_str(&render_column_def(col));

        // Append inline REFERENCES clause if a foreign key targets this column.
        if let Some(fk_list) = fks
            && let Some(fk) = fk_list.iter().find(|fk| fk.column == col.name)
        {
            use std::fmt::Write;
            let _ = write!(
                sql,
                " REFERENCES \"{}\"(\"{}\") ON DELETE {} ON UPDATE {}",
                fk.referenced_table, fk.referenced_column, fk.on_delete, fk.on_update
            );
        }
    }

    if !ct.primary_key.is_empty() {
        sql.push_str(",\n    PRIMARY KEY (");
        sql.push_str(
            &ct.primary_key
                .iter()
                .map(|k| format!("\"{k}\""))
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push(')');
    }

    sql.push_str("\n);\n");
    sql
}

fn render_column_def(col: &ColumnDef) -> String {
    // Map SERIAL to INTEGER (SQLite auto-increments INTEGER PRIMARY KEY automatically)
    let sql_type = if col.sql_type.eq_ignore_ascii_case("SERIAL")
        || col.sql_type.eq_ignore_ascii_case("BIGSERIAL")
    {
        "INTEGER"
    } else {
        &col.sql_type
    };

    let mut s = format!("\"{}\" {}", col.name, sql_type);

    if !col.nullable {
        s.push_str(" NOT NULL");
    }

    if let Some(default) = &col.default
        && !default.is_empty()
    {
        use std::fmt::Write;
        let _ = write!(s, " DEFAULT {default}");
    }

    if col.is_unique {
        s.push_str(" UNIQUE");
    }

    s
}

/// `SQLite` does not support ALTER TABLE ... ALTER COLUMN.
/// Emit a comment explaining that manual table recreation is needed.
fn render_alter_column(table: &str, column: &str, changes: &ColumnChanges) -> String {
    use std::fmt::Write;
    let mut sql = String::new();
    let _ = write!(
        sql,
        "-- SQLite: ALTER COLUMN is not supported. To alter column \"{column}\" on \"{table}\",\n\
         -- you must recreate the table (CREATE TABLE new -> INSERT INTO new SELECT ... -> DROP TABLE old -> ALTER TABLE new RENAME TO old).\n"
    );

    if let Some(new_type) = &changes.sql_type {
        let _ = writeln!(sql, "-- Requested change: type -> {new_type}");
    }

    if let Some(nullable) = changes.nullable {
        if nullable {
            sql.push_str("-- Requested change: DROP NOT NULL\n");
        } else {
            sql.push_str("-- Requested change: SET NOT NULL\n");
        }
    }

    sql
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_create_table() {
        let ct = CreateTable {
            name: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    sql_type: "INTEGER".into(),
                    nullable: false,
                    default: None,
                    is_unique: false,
                },
                ColumnDef {
                    name: "email".into(),
                    sql_type: "TEXT".into(),
                    nullable: false,
                    default: None,
                    is_unique: true,
                },
                ColumnDef {
                    name: "name".into(),
                    sql_type: "TEXT".into(),
                    nullable: true,
                    default: None,
                    is_unique: false,
                },
            ],
            primary_key: vec!["id".into()],
        };

        let sql = render_create_table(&ct, None);
        assert!(sql.contains("CREATE TABLE \"users\""));
        assert!(sql.contains("\"id\" INTEGER NOT NULL"));
        assert!(sql.contains("\"email\" TEXT NOT NULL UNIQUE"));
        assert!(sql.contains("\"name\" TEXT"));
        assert!(sql.contains("PRIMARY KEY (\"id\")"));
    }

    #[test]
    fn test_render_serial_mapped_to_integer() {
        let col = ColumnDef {
            name: "id".into(),
            sql_type: "SERIAL".into(),
            nullable: false,
            default: None,
            is_unique: false,
        };
        let rendered = render_column_def(&col);
        assert!(rendered.contains("INTEGER"));
        assert!(!rendered.contains("SERIAL"));
    }

    #[test]
    fn test_enum_skipped() {
        let step = MigrationStep::CreateEnum {
            name: "role".into(),
            variants: vec!["User".into(), "Admin".into()],
        };
        let sql = SqliteRenderer.render(&[step]);
        assert!(sql.contains("-- SQLite: enum"));
        assert!(!sql.contains("CREATE TYPE"));
    }

    #[test]
    fn test_alter_column_emits_comment() {
        let step = MigrationStep::AlterColumn {
            table: "users".into(),
            column: "email".into(),
            changes: ColumnChanges {
                sql_type: Some("INTEGER".into()),
                nullable: Some(true),
                default: None,
            },
        };
        let sql = SqliteRenderer.render(&[step]);
        assert!(sql.contains("ALTER COLUMN is not supported"));
        assert!(sql.contains("type -> INTEGER"));
        assert!(sql.contains("DROP NOT NULL"));
    }

    #[test]
    fn test_foreign_key_emits_comment_without_create_table() {
        // When AddForeignKey is rendered without a matching CreateTable
        // (e.g., adding FK to an existing table), it emits a comment.
        let step = MigrationStep::AddForeignKey(ForeignKeyDef {
            table: "posts".into(),
            constraint_name: "fk_posts_users".into(),
            column: "author_id".into(),
            referenced_table: "users".into(),
            referenced_column: "id".into(),
            on_delete: "CASCADE".into(),
            on_update: "NO ACTION".into(),
        });
        let sql = SqliteRenderer.render(&[step]);
        assert!(sql.contains("-- SQLite: cannot add foreign key"));
        assert!(sql.contains("\"posts\".\"author_id\""));
    }

    #[test]
    fn test_foreign_key_inline_with_create_table() {
        // When AddForeignKey accompanies a CreateTable for the same table,
        // the FK is rendered inline as a REFERENCES clause.
        let steps = vec![
            MigrationStep::CreateTable(CreateTable {
                name: "posts".into(),
                columns: vec![
                    ColumnDef {
                        name: "id".into(),
                        sql_type: "TEXT".into(),
                        nullable: false,
                        default: None,
                        is_unique: false,
                    },
                    ColumnDef {
                        name: "author_id".into(),
                        sql_type: "TEXT".into(),
                        nullable: false,
                        default: None,
                        is_unique: false,
                    },
                ],
                primary_key: vec!["id".into()],
            }),
            MigrationStep::AddForeignKey(ForeignKeyDef {
                table: "posts".into(),
                constraint_name: "fk_posts_users".into(),
                column: "author_id".into(),
                referenced_table: "users".into(),
                referenced_column: "id".into(),
                on_delete: "CASCADE".into(),
                on_update: "NO ACTION".into(),
            }),
        ];
        let sql = SqliteRenderer.render(&steps);
        assert!(
            sql.contains("REFERENCES \"users\"(\"id\") ON DELETE CASCADE ON UPDATE NO ACTION"),
            "FK should be inlined as REFERENCES. Got:\n{sql}"
        );
        assert!(
            !sql.contains("-- SQLite: cannot add foreign key"),
            "Should NOT emit FK comment when inlined. Got:\n{sql}"
        );
    }

    #[test]
    fn test_unique_constraint_as_index() {
        let step = MigrationStep::AddUniqueConstraint {
            table: "users".into(),
            name: "uq_users_email".into(),
            columns: vec!["email".into()],
        };
        let sql = SqliteRenderer.render(&[step]);
        assert!(sql.contains("CREATE UNIQUE INDEX \"uq_users_email\" ON \"users\" (\"email\")"));
    }
}
