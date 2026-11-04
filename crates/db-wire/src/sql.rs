use crate::DbDriverKind;
use mem_engine::QueryResultPager;
use std::collections::{BTreeSet, HashMap};

/// Database-neutral row edits. Row indexes refer to the original result pager.
#[derive(Default)]
pub struct RowChanges {
    pub changes: HashMap<(u64, usize), Option<String>>,
    pub inserted_rows: Vec<Vec<Option<String>>>,
    pub inserted_nulls: BTreeSet<(usize, usize)>,
    pub deleted_rows: BTreeSet<u64>,
}

#[derive(Clone, Copy)]
pub struct SqlDialect(pub DbDriverKind);
impl SqlDialect {
    pub fn quote_ident(self, name: &str) -> String {
        match self.0 {
            DbDriverKind::MySql | DbDriverKind::MariaDb => format!("`{}`", name.replace('`', "``")),
            DbDriverKind::SqlServer => format!("[{}]", name.replace(']', "]]")),
            _ => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }
    pub fn qualified_table(self, schema: &str, table: &str) -> String {
        if self.0 == DbDriverKind::Sqlite {
            self.quote_ident(table)
        } else {
            format!("{}.{}", self.quote_ident(schema), self.quote_ident(table))
        }
    }
    pub fn literal(self, value: Option<&str>) -> String {
        match value {
            None => "NULL".into(),
            Some(v) => {
                let v = if matches!(self.0, DbDriverKind::MySql | DbDriverKind::MariaDb) {
                    v.replace('\\', "\\\\")
                } else {
                    v.into()
                };
                format!(
                    "{}'{}'",
                    if self.0 == DbDriverKind::SqlServer {
                        "N"
                    } else {
                        ""
                    },
                    v.replace('\'', "''")
                )
            }
        }
    }
    pub fn default_insert(self, target: &str) -> anyhow::Result<String> {
        match self.0 {
            DbDriverKind::MySql | DbDriverKind::MariaDb => {
                Ok(format!("INSERT INTO {target} () VALUES ();"))
            }
            DbDriverKind::Oracle | DbDriverKind::Db2 => anyhow::bail!(
                "Default-only inserts are not supported for this provider; set at least one column"
            ),
            _ => Ok(format!("INSERT INTO {target} DEFAULT VALUES;")),
        }
    }
}

pub fn quote_ident(kind: DbDriverKind, name: &str) -> String {
    SqlDialect(kind).quote_ident(name)
}
pub fn table_page_suffix(kind: DbDriverKind, size: usize, offset: u64, ordered: bool) -> String {
    match kind {
        DbDriverKind::SqlServer => format!(
            "{} OFFSET {offset} ROWS FETCH NEXT {size} ROWS ONLY",
            if ordered {
                ""
            } else {
                " ORDER BY (SELECT NULL)"
            }
        ),
        DbDriverKind::Oracle | DbDriverKind::Db2 => {
            format!(" OFFSET {offset} ROWS FETCH NEXT {size} ROWS ONLY")
        }
        _ if offset > 0 => format!(" LIMIT {size} OFFSET {offset}"),
        _ => format!(" LIMIT {size}"),
    }
}
impl RowChanges {
    /// Generates the statements that apply every staged change: UPDATEs for edited rows,
    /// INSERTs for new rows and DELETEs for deleted rows. Rows are identified by the
    /// `key_cols` values, or by all of their original values when there is no key.
    pub fn plan_sql(
        &self,
        target: &str,
        pager: &QueryResultPager,
        key_cols: &[usize],
        dialect: SqlDialect,
    ) -> anyhow::Result<Vec<String>> {
        anyhow::ensure!(
            !pager.columns.is_empty(),
            "Cannot save rows without column metadata"
        );
        anyhow::ensure!(
            key_cols.iter().all(|&c| c < pager.columns.len()),
            "Invalid row key column"
        );
        anyhow::ensure!(
            self.changes
                .keys()
                .all(|&(row, col)| row < pager.total_rows() && col < pager.columns.len()),
            "Invalid edited cell"
        );
        anyhow::ensure!(
            self.deleted_rows
                .iter()
                .all(|&row| row < pager.total_rows()),
            "Invalid deleted row"
        );
        anyhow::ensure!(
            self.inserted_rows
                .iter()
                .all(|row| row.len() == pager.columns.len()),
            "Inserted row does not match result columns"
        );
        let col_name = |c: usize| {
            let name = pager
                .columns
                .get(c)
                .map(|col| col.name.as_str())
                .unwrap_or("col");
            dialect.quote_ident(name)
        };
        let where_clause = |row: u64| {
            let all_cols: Vec<usize> = (0..pager.columns.len()).collect();
            let cols = if key_cols.is_empty() {
                &all_cols[..]
            } else {
                key_cols
            };
            cols.iter()
                .map(|&c| match pager.get_cell(row, c) {
                    Some(bytes) => format!(
                        "{} = {}",
                        col_name(c),
                        dialect.literal(Some(&String::from_utf8_lossy(&bytes)))
                    ),
                    None => format!("{} IS NULL", col_name(c)),
                })
                .collect::<Vec<_>>()
                .join(" AND ")
        };

        let mut statements = Vec::new();

        let mut edited_rows: HashMap<u64, Vec<(usize, Option<&str>)>> = HashMap::new();
        for (key, cell) in &self.changes {
            if !self.deleted_rows.contains(&key.0) {
                edited_rows
                    .entry(key.0)
                    .or_default()
                    .push((key.1, cell.as_deref()));
            }
        }
        let mut edited_rows: Vec<_> = edited_rows.into_iter().collect();
        edited_rows.sort_by_key(|(row, _)| *row);
        for (row, mut edits) in edited_rows {
            edits.sort_by_key(|(c, _)| *c);
            let set_clause = edits
                .iter()
                .map(|&(c, v)| format!("{} = {}", col_name(c), dialect.literal(v)))
                .collect::<Vec<_>>()
                .join(", ");
            statements.push(format!(
                "UPDATE {target} SET {set_clause} WHERE {};",
                where_clause(row)
            ));
        }

        anyhow::ensure!(
            self.inserted_nulls.iter().all(|&(row, column)| self
                .inserted_rows
                .get(row)
                .and_then(|values| values.get(column))
                .is_some_and(Option::is_none)),
            "Invalid explicit NULL in inserted row"
        );
        for (index, row) in self.inserted_rows.iter().enumerate() {
            let set: Vec<(usize, Option<&str>)> = row
                .iter()
                .enumerate()
                .filter_map(|(c, v)| {
                    (v.is_some() || self.inserted_nulls.contains(&(index, c)))
                        .then_some((c, v.as_deref()))
                })
                .collect();
            if set.is_empty() {
                statements.push(dialect.default_insert(target)?);
            } else {
                let cols = set
                    .iter()
                    .map(|&(c, _)| col_name(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let vals = set
                    .iter()
                    .map(|&(_, v)| dialect.literal(v))
                    .collect::<Vec<_>>()
                    .join(", ");
                statements.push(format!("INSERT INTO {target} ({cols}) VALUES ({vals});"));
            }
        }

        for &row in &self.deleted_rows {
            statements.push(format!("DELETE FROM {target} WHERE {};", where_clause(row)));
        }

        Ok(statements)
    }
}

mod ddl;
pub use ddl::generate_table_ddl;
pub mod schema_ddl;
pub use schema_ddl::{ColumnDef, SchemaAction, generate_schema_ddl, is_ddl_transactional};

#[cfg(test)]
mod tests {
    use super::{RowChanges, SqlDialect, table_page_suffix};
    use crate::{ColumnMeta, DataType, DbDriverKind};
    use mem_engine::QueryResultPager;

    fn users() -> QueryResultPager {
        let pager = QueryResultPager::new(vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("username", DataType::Text),
        ])
        .unwrap();
        pager
            .append_row([Some(b"42".as_slice()), Some(b"old_name".as_slice())])
            .unwrap();
        pager.append_row([Some(b"43".as_slice()), None]).unwrap();
        pager
    }
    #[test]
    fn explicit_insert_null_is_distinct_from_an_omitted_default() {
        use crate::{DatabaseDriver, SqliteDriver};
        crate::get_runtime().block_on(async {
            let driver = SqliteDriver::new_memory();
            driver
                .execute_query(
                    "CREATE TABLE users (id INT DEFAULT 7, username TEXT DEFAULT 'generated')",
                )
                .await
                .unwrap();
            let mut request = RowChanges::default();
            request.inserted_rows.push(vec![None, None]);
            request.inserted_nulls.insert((0, 1));
            let statements = request
                .plan_sql("users", &users(), &[0], SqlDialect(DbDriverKind::Sqlite))
                .unwrap();
            assert_eq!(
                statements,
                ["INSERT INTO users (\"username\") VALUES (NULL);"]
            );
            driver.apply_row_changes(&statements).await.unwrap();
            let (pager, _, _) = driver
                .execute_query("SELECT id, username FROM users")
                .await
                .unwrap();
            assert_eq!(pager.get_cell(0, 0).unwrap(), b"7");
            assert_eq!(pager.get_cell(0, 1), None);
        });
    }

    #[test]
    fn writes_preserve_order_keys_null_and_escaping() {
        let pager = users();
        let mut request = RowChanges::default();
        request.changes.insert((0, 1), Some("it's new".into()));
        assert_eq!(
            request
                .plan_sql(
                    "\"public\".\"users\"",
                    &pager,
                    &[0],
                    SqlDialect(DbDriverKind::Postgres)
                )
                .unwrap(),
            ["UPDATE \"public\".\"users\" SET \"username\" = 'it''s new' WHERE \"id\" = '42';"]
        );
        request.changes.insert((0, 1), None);
        assert!(
            request
                .plan_sql("users", &pager, &[0], SqlDialect(DbDriverKind::Sqlite))
                .unwrap()[0]
                .contains("\"username\" = NULL")
        );
        request.changes.clear();
        request.inserted_rows = vec![vec![None, Some("new\\user".into())], vec![None, None]];
        request.deleted_rows.insert(1);
        request.changes.insert((1, 1), Some("ignored".into()));
        assert_eq!(
            request
                .plan_sql("`users`", &pager, &[], SqlDialect(DbDriverKind::MySql))
                .unwrap(),
            [
                "INSERT INTO `users` (`username`) VALUES ('new\\\\user');",
                "INSERT INTO `users` () VALUES ();",
                "DELETE FROM `users` WHERE `id` = '43' AND `username` IS NULL;",
            ]
        );
    }
    #[test]
    fn provider_syntax_is_explicit() {
        for kind in [
            DbDriverKind::Postgres,
            DbDriverKind::MySql,
            DbDriverKind::MariaDb,
            DbDriverKind::Sqlite,
            DbDriverKind::SqlServer,
            DbDriverKind::Oracle,
            DbDriverKind::Db2,
        ] {
            let d = SqlDialect(kind);
            let quoted = d.quote_ident("a`\"]b");
            match kind {
                DbDriverKind::MySql | DbDriverKind::MariaDb => assert_eq!(quoted, "`a``\"]b`"),
                DbDriverKind::SqlServer => assert_eq!(quoted, "[a`\"]]b]"),
                _ => assert_eq!(quoted, "\"a`\"\"]b\""),
            }
            assert_eq!(d.literal(None), "NULL");
            assert!(d.literal(Some("it's\\text")).contains("it''s"));
            assert_eq!(
                d.default_insert("t").is_err(),
                matches!(kind, DbDriverKind::Oracle | DbDriverKind::Db2)
            );
            let suffix = table_page_suffix(kind, 20, 40, true);
            if matches!(
                kind,
                DbDriverKind::SqlServer | DbDriverKind::Oracle | DbDriverKind::Db2
            ) {
                assert_eq!(suffix, " OFFSET 40 ROWS FETCH NEXT 20 ROWS ONLY");
            } else {
                assert_eq!(suffix, " LIMIT 20 OFFSET 40");
            }
            let mut request = RowChanges::default();
            request.changes.insert((0, 1), Some("new".into()));
            let statement = request
                .plan_sql(&d.qualified_table("s", "t"), &users(), &[0], d)
                .unwrap()
                .remove(0);
            assert!(statement.starts_with("UPDATE "));
            assert!(statement.contains(&format!(
                "{} = {}",
                d.quote_ident("username"),
                d.literal(Some("new"))
            )));
        }
        assert_eq!(
            SqlDialect(DbDriverKind::Sqlite).qualified_table("ignored", "table"),
            "\"table\""
        );
        assert_eq!(
            SqlDialect(DbDriverKind::SqlServer).literal(Some("Tiếng Việt")),
            "N'Tiếng Việt'"
        );
    }
    #[test]
    fn invalid_cell_indexes_do_not_generate_a_write() {
        let mut request = RowChanges::default();
        request.changes.insert((99, 0), Some("x".into()));
        assert!(
            request
                .plan_sql("t", &users(), &[0], SqlDialect(DbDriverKind::Postgres))
                .is_err()
        );
    }
}
