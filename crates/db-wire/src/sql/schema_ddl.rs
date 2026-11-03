use super::SqlDialect;
use crate::DbDriverKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_pk: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaAction {
    TruncateTable {
        schema: String,
        table: String,
        cascade: bool,
    },
    DropTable {
        schema: String,
        table: String,
        cascade: bool,
    },
    RenameTable {
        schema: String,
        table: String,
        new_name: String,
    },
    CreateTable {
        schema: String,
        table: String,
        columns: Vec<ColumnDef>,
    },
    DropView {
        schema: String,
        view: String,
        cascade: bool,
    },
    RenameView {
        schema: String,
        view: String,
        new_name: String,
    },
    CreateView {
        schema: String,
        view: String,
        definition: String,
    },
    AddColumn {
        schema: String,
        table: String,
        column: String,
        data_type: String,
        nullable: bool,
        default_val: Option<String>,
    },
    DropColumn {
        schema: String,
        table: String,
        column: String,
        cascade: bool,
    },
    RenameColumn {
        schema: String,
        table: String,
        old_name: String,
        new_name: String,
    },
    DropConstraint {
        schema: String,
        table: String,
        constraint: String,
        constraint_type: Option<String>,
        cascade: bool,
    },
    DropIndex {
        schema: String,
        table: String,
        index: String,
        cascade: bool,
    },
}

/// Returns whether the target database engine executes DDL inside an ACID transaction.
/// In MySQL/MariaDB, DDL causes an implicit COMMIT and cannot be rolled back.
pub fn is_ddl_transactional(kind: DbDriverKind) -> bool {
    matches!(kind, DbDriverKind::Postgres | DbDriverKind::Sqlite)
}

fn validate_identifier(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Identifier name cannot be empty".into());
    }
    if trimmed.contains('\0') {
        return Err("Identifier name cannot contain null characters".into());
    }
    Ok(())
}

/// Generates dialect-aware, properly quoted DDL statement(s) for the specified schema action.
pub fn generate_schema_ddl(kind: DbDriverKind, action: &SchemaAction) -> Result<String, String> {
    let dialect = SqlDialect(kind);
    match action {
        SchemaAction::TruncateTable {
            schema,
            table,
            cascade,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            match kind {
                DbDriverKind::Sqlite => Ok(format!("DELETE FROM {};", dialect.quote_ident(table))),
                DbDriverKind::Postgres => {
                    let cascade_clause = if *cascade { " CASCADE" } else { "" };
                    Ok(format!(
                        "TRUNCATE TABLE {}{};",
                        dialect.qualified_table(schema, table),
                        cascade_clause
                    ))
                }
                _ => Ok(format!(
                    "TRUNCATE TABLE {};",
                    dialect.qualified_table(schema, table)
                )),
            }
        }
        SchemaAction::DropTable {
            schema,
            table,
            cascade,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            match kind {
                DbDriverKind::Sqlite => Ok(format!("DROP TABLE {};", dialect.quote_ident(table))),
                DbDriverKind::Postgres => {
                    let cascade_clause = if *cascade { " CASCADE" } else { "" };
                    Ok(format!(
                        "DROP TABLE {}{};",
                        dialect.qualified_table(schema, table),
                        cascade_clause
                    ))
                }
                _ => Ok(format!(
                    "DROP TABLE {};",
                    dialect.qualified_table(schema, table)
                )),
            }
        }
        SchemaAction::RenameTable {
            schema,
            table,
            new_name,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            validate_identifier(new_name)?;
            match kind {
                DbDriverKind::Sqlite => Ok(format!(
                    "ALTER TABLE {} RENAME TO {};",
                    dialect.quote_ident(table),
                    dialect.quote_ident(new_name)
                )),
                DbDriverKind::MySql | DbDriverKind::MariaDb => Ok(format!(
                    "RENAME TABLE {} TO {};",
                    dialect.qualified_table(schema, table),
                    dialect.qualified_table(schema, new_name)
                )),
                _ => Ok(format!(
                    "ALTER TABLE {} RENAME TO {};",
                    dialect.qualified_table(schema, table),
                    dialect.quote_ident(new_name)
                )),
            }
        }
        SchemaAction::CreateTable {
            schema,
            table,
            columns,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            if columns.is_empty() {
                return Err("Table must have at least one column definition".into());
            }
            let mut lines = Vec::new();
            let mut pk_cols = Vec::new();
            for col in columns {
                validate_identifier(&col.name)?;
                if col.data_type.trim().is_empty() {
                    return Err(format!(
                        "Data type for column '{}' cannot be empty",
                        col.name
                    ));
                }
                let null_clause = if col.nullable { "NULL" } else { "NOT NULL" };
                lines.push(format!(
                    "    {} {} {}",
                    dialect.quote_ident(&col.name),
                    col.data_type.trim(),
                    null_clause
                ));
                if col.is_pk {
                    pk_cols.push(dialect.quote_ident(&col.name));
                }
            }
            if !pk_cols.is_empty() {
                lines.push(format!("    PRIMARY KEY ({})", pk_cols.join(", ")));
            }
            let target = if kind == DbDriverKind::Sqlite {
                dialect.quote_ident(table)
            } else {
                dialect.qualified_table(schema, table)
            };
            Ok(format!(
                "CREATE TABLE {} (\n{}\n);",
                target,
                lines.join(",\n")
            ))
        }
        SchemaAction::DropView {
            schema,
            view,
            cascade,
        } => {
            validate_identifier(schema)?;
            validate_identifier(view)?;
            match kind {
                DbDriverKind::Sqlite => Ok(format!("DROP VIEW {};", dialect.quote_ident(view))),
                DbDriverKind::Postgres => {
                    let cascade_clause = if *cascade { " CASCADE" } else { "" };
                    Ok(format!(
                        "DROP VIEW {}{};",
                        dialect.qualified_table(schema, view),
                        cascade_clause
                    ))
                }
                _ => Ok(format!(
                    "DROP VIEW {};",
                    dialect.qualified_table(schema, view)
                )),
            }
        }
        SchemaAction::RenameView {
            schema,
            view,
            new_name,
        } => {
            validate_identifier(schema)?;
            validate_identifier(view)?;
            validate_identifier(new_name)?;
            match kind {
                DbDriverKind::Sqlite => Ok(format!(
                    "ALTER TABLE {} RENAME TO {};",
                    dialect.quote_ident(view),
                    dialect.quote_ident(new_name)
                )),
                DbDriverKind::MySql | DbDriverKind::MariaDb => Ok(format!(
                    "RENAME TABLE {} TO {};",
                    dialect.qualified_table(schema, view),
                    dialect.qualified_table(schema, new_name)
                )),
                _ => Ok(format!(
                    "ALTER VIEW {} RENAME TO {};",
                    dialect.qualified_table(schema, view),
                    dialect.quote_ident(new_name)
                )),
            }
        }
        SchemaAction::CreateView {
            schema,
            view,
            definition,
        } => {
            validate_identifier(schema)?;
            validate_identifier(view)?;
            let trimmed = definition.trim();
            if trimmed.is_empty() {
                return Err("View query definition cannot be empty".into());
            }
            let target = if kind == DbDriverKind::Sqlite {
                dialect.quote_ident(view)
            } else {
                dialect.qualified_table(schema, view)
            };
            Ok(format!("CREATE VIEW {} AS {};", target, trimmed))
        }
        SchemaAction::AddColumn {
            schema,
            table,
            column,
            data_type,
            nullable,
            default_val,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            validate_identifier(column)?;
            if data_type.trim().is_empty() {
                return Err("Column data type cannot be empty".into());
            }
            let target = if kind == DbDriverKind::Sqlite {
                dialect.quote_ident(table)
            } else {
                dialect.qualified_table(schema, table)
            };
            let default_clause = default_val
                .as_deref()
                .map(|d| format!(" DEFAULT {d}"))
                .unwrap_or_default();
            let null_clause = match (kind, *nullable) {
                (DbDriverKind::Sqlite, true) => "",
                (DbDriverKind::Sqlite, false) => " NOT NULL",
                (_, true) => " NULL",
                (_, false) => " NOT NULL",
            };
            Ok(format!(
                "ALTER TABLE {} ADD COLUMN {} {}{}{};",
                target,
                dialect.quote_ident(column),
                data_type.trim(),
                default_clause,
                null_clause
            ))
        }
        SchemaAction::DropColumn {
            schema,
            table,
            column,
            cascade,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            validate_identifier(column)?;
            let target = if kind == DbDriverKind::Sqlite {
                dialect.quote_ident(table)
            } else {
                dialect.qualified_table(schema, table)
            };
            let cascade_clause = if *cascade && kind == DbDriverKind::Postgres {
                " CASCADE"
            } else {
                ""
            };
            Ok(format!(
                "ALTER TABLE {} DROP COLUMN {}{};",
                target,
                dialect.quote_ident(column),
                cascade_clause
            ))
        }
        SchemaAction::RenameColumn {
            schema,
            table,
            old_name,
            new_name,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            validate_identifier(old_name)?;
            validate_identifier(new_name)?;
            let target = if kind == DbDriverKind::Sqlite {
                dialect.quote_ident(table)
            } else {
                dialect.qualified_table(schema, table)
            };
            Ok(format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {};",
                target,
                dialect.quote_ident(old_name),
                dialect.quote_ident(new_name)
            ))
        }
        SchemaAction::DropConstraint {
            schema,
            table,
            constraint,
            constraint_type,
            cascade,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            validate_identifier(constraint)?;
            if kind == DbDriverKind::Sqlite {
                return Err("SQLite does not support DROP CONSTRAINT directly".into());
            }
            let target = dialect.qualified_table(schema, table);
            if matches!(kind, DbDriverKind::MySql | DbDriverKind::MariaDb) {
                if let Some(ctype) = constraint_type {
                    let upper = ctype.to_uppercase();
                    if upper.contains("PRIMARY") {
                        return Ok(format!("ALTER TABLE {} DROP PRIMARY KEY;", target));
                    } else if upper.contains("FOREIGN") {
                        return Ok(format!(
                            "ALTER TABLE {} DROP FOREIGN KEY {};",
                            target,
                            dialect.quote_ident(constraint)
                        ));
                    } else if upper.contains("UNIQUE") {
                        return Ok(format!(
                            "ALTER TABLE {} DROP INDEX {};",
                            target,
                            dialect.quote_ident(constraint)
                        ));
                    }
                }
                return Ok(format!(
                    "ALTER TABLE {} DROP CONSTRAINT {};",
                    target,
                    dialect.quote_ident(constraint)
                ));
            }
            let cascade_clause = if *cascade && kind == DbDriverKind::Postgres {
                " CASCADE"
            } else {
                ""
            };
            Ok(format!(
                "ALTER TABLE {} DROP CONSTRAINT {}{};",
                target,
                dialect.quote_ident(constraint),
                cascade_clause
            ))
        }
        SchemaAction::DropIndex {
            schema,
            table,
            index,
            cascade,
        } => {
            validate_identifier(schema)?;
            validate_identifier(table)?;
            validate_identifier(index)?;
            match kind {
                DbDriverKind::Sqlite => Ok(format!("DROP INDEX {};", dialect.quote_ident(index))),
                DbDriverKind::Postgres => {
                    let cascade_clause = if *cascade { " CASCADE" } else { "" };
                    Ok(format!(
                        "DROP INDEX {}{};",
                        dialect.qualified_table(schema, index),
                        cascade_clause
                    ))
                }
                DbDriverKind::MySql | DbDriverKind::MariaDb => Ok(format!(
                    "ALTER TABLE {} DROP INDEX {};",
                    dialect.qualified_table(schema, table),
                    dialect.quote_ident(index)
                )),
                DbDriverKind::SqlServer => Ok(format!(
                    "DROP INDEX {} ON {};",
                    dialect.quote_ident(index),
                    dialect.qualified_table(schema, table)
                )),
                _ => Ok(format!(
                    "ALTER TABLE {} DROP INDEX {};",
                    dialect.qualified_table(schema, table),
                    dialect.quote_ident(index)
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_table_dialect_variations() {
        let action = SchemaAction::TruncateTable {
            schema: "public".into(),
            table: "logs".into(),
            cascade: true,
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &action).unwrap(),
            "TRUNCATE TABLE \"public\".\"logs\" CASCADE;"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Sqlite, &action).unwrap(),
            "DELETE FROM \"logs\";"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::MySql, &action).unwrap(),
            "TRUNCATE TABLE `public`.`logs`;"
        );
    }

    #[test]
    fn drop_table_and_view_dialect_variations() {
        let drop_table = SchemaAction::DropTable {
            schema: "s".into(),
            table: "t".into(),
            cascade: true,
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &drop_table).unwrap(),
            "DROP TABLE \"s\".\"t\" CASCADE;"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Sqlite, &drop_table).unwrap(),
            "DROP TABLE \"t\";"
        );

        let drop_view = SchemaAction::DropView {
            schema: "s".into(),
            view: "v".into(),
            cascade: false,
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &drop_view).unwrap(),
            "DROP VIEW \"s\".\"v\";"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Sqlite, &drop_view).unwrap(),
            "DROP VIEW \"v\";"
        );
    }

    #[test]
    fn rename_table_dialect_variations() {
        let rename = SchemaAction::RenameTable {
            schema: "s".into(),
            table: "old_tbl".into(),
            new_name: "new_tbl".into(),
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &rename).unwrap(),
            "ALTER TABLE \"s\".\"old_tbl\" RENAME TO \"new_tbl\";"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Sqlite, &rename).unwrap(),
            "ALTER TABLE \"old_tbl\" RENAME TO \"new_tbl\";"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::MySql, &rename).unwrap(),
            "RENAME TABLE `s`.`old_tbl` TO `s`.`new_tbl`;"
        );
    }

    #[test]
    fn column_ddl_generation() {
        let add = SchemaAction::AddColumn {
            schema: "s".into(),
            table: "t".into(),
            column: "age".into(),
            data_type: "integer".into(),
            nullable: false,
            default_val: Some("0".into()),
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &add).unwrap(),
            "ALTER TABLE \"s\".\"t\" ADD COLUMN \"age\" integer DEFAULT 0 NOT NULL;"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Sqlite, &add).unwrap(),
            "ALTER TABLE \"t\" ADD COLUMN \"age\" integer DEFAULT 0 NOT NULL;"
        );

        let drop_col = SchemaAction::DropColumn {
            schema: "s".into(),
            table: "t".into(),
            column: "temp".into(),
            cascade: true,
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &drop_col).unwrap(),
            "ALTER TABLE \"s\".\"t\" DROP COLUMN \"temp\" CASCADE;"
        );

        let rename_col = SchemaAction::RenameColumn {
            schema: "s".into(),
            table: "t".into(),
            old_name: "c1".into(),
            new_name: "c2".into(),
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &rename_col).unwrap(),
            "ALTER TABLE \"s\".\"t\" RENAME COLUMN \"c1\" TO \"c2\";"
        );
    }

    #[test]
    fn create_table_ddl_generation() {
        let create = SchemaAction::CreateTable {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                    is_pk: true,
                },
                ColumnDef {
                    name: "email".into(),
                    data_type: "text".into(),
                    nullable: true,
                    is_pk: false,
                },
            ],
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &create).unwrap();
        assert!(ddl.contains("CREATE TABLE \"public\".\"users\""));
        assert!(ddl.contains("\"id\" bigint NOT NULL"));
        assert!(ddl.contains("\"email\" text NULL"));
        assert!(ddl.contains("PRIMARY KEY (\"id\")"));
    }

    #[test]
    fn identifier_validation_rejects_empty_and_nulls() {
        let invalid = SchemaAction::DropTable {
            schema: "public".into(),
            table: "".into(),
            cascade: false,
        };
        assert!(generate_schema_ddl(DbDriverKind::Postgres, &invalid).is_err());

        let null_char = SchemaAction::RenameTable {
            schema: "public".into(),
            table: "test\0bad".into(),
            new_name: "good".into(),
        };
        assert!(generate_schema_ddl(DbDriverKind::Postgres, &null_char).is_err());
    }

    #[test]
    fn drop_constraint_and_index_dialect_variations() {
        let drop_idx = SchemaAction::DropIndex {
            schema: "mydb".into(),
            table: "users".into(),
            index: "idx_users_email".into(),
            cascade: false,
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &drop_idx).unwrap(),
            "DROP INDEX \"mydb\".\"idx_users_email\";"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Sqlite, &drop_idx).unwrap(),
            "DROP INDEX \"idx_users_email\";"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::MySql, &drop_idx).unwrap(),
            "ALTER TABLE `mydb`.`users` DROP INDEX `idx_users_email`;"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::SqlServer, &drop_idx).unwrap(),
            "DROP INDEX [idx_users_email] ON [mydb].[users];"
        );

        let drop_fk = SchemaAction::DropConstraint {
            schema: "mydb".into(),
            table: "orders".into(),
            constraint: "fk_user".into(),
            constraint_type: Some("FOREIGN KEY".into()),
            cascade: false,
        };
        assert_eq!(
            generate_schema_ddl(DbDriverKind::MySql, &drop_fk).unwrap(),
            "ALTER TABLE `mydb`.`orders` DROP FOREIGN KEY `fk_user`;"
        );
        assert_eq!(
            generate_schema_ddl(DbDriverKind::Postgres, &drop_fk).unwrap(),
            "ALTER TABLE \"mydb\".\"orders\" DROP CONSTRAINT \"fk_user\";"
        );
        assert!(generate_schema_ddl(DbDriverKind::Sqlite, &drop_fk).is_err());
    }

    #[test]
    fn transactional_check() {
        assert!(is_ddl_transactional(DbDriverKind::Postgres));
        assert!(is_ddl_transactional(DbDriverKind::Sqlite));
        assert!(!is_ddl_transactional(DbDriverKind::MySql));
        assert!(!is_ddl_transactional(DbDriverKind::MariaDb));
    }
}
