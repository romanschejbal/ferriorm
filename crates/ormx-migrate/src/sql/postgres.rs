//! PostgreSQL SQL renderer for migration steps.
//!
//! Generates PostgreSQL-specific DDL: `CREATE TYPE` for enums,
//! `ALTER TABLE ... ALTER COLUMN` for column modifications, `$N` parameter
//! placeholders, and `CASCADE` on `DROP TABLE`.

use super::SqlRenderer;
use crate::diff::*;

pub struct PostgresRenderer;

impl SqlRenderer for PostgresRenderer {
    fn render(&self, steps: &[MigrationStep]) -> String {
        let mut sql = String::new();
        for step in steps {
            sql.push_str(&render_step(step));
            sql.push('\n');
        }
        sql
    }
}

fn render_step(step: &MigrationStep) -> String {
    match step {
        MigrationStep::CreateEnum { name, variants } => {
            let vals = variants
                .iter()
                .map(|v| format!("'{}'", v.to_lowercase()))
                .collect::<Vec<_>>()
                .join(", ");
            format!("CREATE TYPE \"{name}\" AS ENUM ({vals});\n")
        }
        MigrationStep::DropEnum { name } => {
            format!("DROP TYPE IF EXISTS \"{name}\";\n")
        }
        MigrationStep::AddEnumVariant { enum_name, variant } => {
            format!(
                "ALTER TYPE \"{enum_name}\" ADD VALUE '{}';\n",
                variant.to_lowercase()
            )
        }
        MigrationStep::CreateTable(ct) => render_create_table(ct),
        MigrationStep::DropTable { name } => {
            format!("DROP TABLE IF EXISTS \"{name}\" CASCADE;\n")
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
        MigrationStep::AddForeignKey(fk) => {
            format!(
                "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" FOREIGN KEY (\"{}\") REFERENCES \"{}\"(\"{}\") ON DELETE {} ON UPDATE {};\n",
                fk.table,
                fk.constraint_name,
                fk.column,
                fk.referenced_table,
                fk.referenced_column,
                fk.on_delete,
                fk.on_update
            )
        }
        MigrationStep::DropForeignKey { table, name } => {
            format!("ALTER TABLE \"{table}\" DROP CONSTRAINT IF EXISTS \"{name}\";\n")
        }
        MigrationStep::AddUniqueConstraint {
            table,
            name,
            columns,
        } => {
            let cols = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ALTER TABLE \"{table}\" ADD CONSTRAINT \"{name}\" UNIQUE ({cols});\n")
        }
        MigrationStep::DropUniqueConstraint { table, name } => {
            format!("ALTER TABLE \"{table}\" DROP CONSTRAINT IF EXISTS \"{name}\";\n")
        }
    }
}

fn render_create_table(ct: &CreateTable) -> String {
    let mut sql = format!("CREATE TABLE \"{}\" (\n", ct.name);

    for (i, col) in ct.columns.iter().enumerate() {
        if i > 0 {
            sql.push_str(",\n");
        }
        sql.push_str("    ");
        sql.push_str(&render_column_def(col));
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
    let mut s = format!("\"{}\" {}", col.name, col.sql_type);

    if !col.nullable {
        s.push_str(" NOT NULL");
    }

    if let Some(default) = &col.default
        && !default.is_empty()
    {
        s.push_str(&format!(" DEFAULT {default}"));
    }

    if col.is_unique {
        s.push_str(" UNIQUE");
    }

    s
}

fn render_alter_column(table: &str, column: &str, changes: &ColumnChanges) -> String {
    let mut sql = String::new();

    if let Some(new_type) = &changes.sql_type {
        sql.push_str(&format!(
            "ALTER TABLE \"{table}\" ALTER COLUMN \"{column}\" TYPE {new_type};\n"
        ));
    }

    if let Some(nullable) = changes.nullable {
        if nullable {
            sql.push_str(&format!(
                "ALTER TABLE \"{table}\" ALTER COLUMN \"{column}\" DROP NOT NULL;\n"
            ));
        } else {
            sql.push_str(&format!(
                "ALTER TABLE \"{table}\" ALTER COLUMN \"{column}\" SET NOT NULL;\n"
            ));
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
                    sql_type: "TEXT".into(),
                    nullable: false,
                    default: Some("gen_random_uuid()".into()),
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

        let sql = render_create_table(&ct);
        assert!(sql.contains("CREATE TABLE \"users\""));
        assert!(sql.contains("\"id\" TEXT NOT NULL DEFAULT gen_random_uuid()"));
        assert!(sql.contains("\"email\" TEXT NOT NULL UNIQUE"));
        assert!(sql.contains("\"name\" TEXT"));
        assert!(sql.contains("PRIMARY KEY (\"id\")"));
    }

    #[test]
    fn test_render_create_enum() {
        let step = MigrationStep::CreateEnum {
            name: "role".into(),
            variants: vec!["User".into(), "Admin".into()],
        };
        let sql = PostgresRenderer.render(&[step]);
        assert!(sql.contains("CREATE TYPE \"role\" AS ENUM ('user', 'admin')"));
    }
}
