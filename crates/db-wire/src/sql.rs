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
        let cell_literal = |column: usize, value: Option<&str>| -> anyhow::Result<String> {
            match value {
                Some(text) if pager.columns[column].data_type == crate::DataType::Bytea => {
                    let bytes = crate::value::parse_hex(text)?;
                    anyhow::ensure!(
                        dialect.0 != DbDriverKind::Oracle || !bytes.is_empty(),
                        "Empty binary edits are unavailable for Oracle without native RAW/BLOB type information; SQL NULL remains explicit"
                    );
                    Ok(crate::value::binary_literal(&bytes, dialect.0))
                }
                _ => Ok(value
                    .map(|text| crate::filter::value_literal(dialect.0, text))
                    .unwrap_or_else(|| "NULL".into())),
            }
        };
        let where_clause = |row: u64| -> anyhow::Result<String> {
            let all_cols: Vec<usize> = (0..pager.columns.len()).collect();
            let cols = if key_cols.is_empty() {
                &all_cols[..]
            } else {
                key_cols
            };
            Ok(cols
                .iter()
                .map(|&c| -> anyhow::Result<String> {
                    Ok(match pager.get_cell(row, c) {
                        Some(bytes) => {
                            let literal = if pager.columns[c].data_type == crate::DataType::Bytea {
                                crate::value::binary_literal(
                                    &crate::value::source_binary(&bytes, dialect.0)?,
                                    dialect.0,
                                )
                            } else {
                                crate::filter::value_literal(
                                    dialect.0,
                                    std::str::from_utf8(&bytes)?,
                                )
                            };
                            format!("{} = {}", col_name(c), literal)
                        }
                        None => format!("{} IS NULL", col_name(c)),
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?
                .join(" AND "))
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
                .map(|&(c, v)| Ok(format!("{} = {}", col_name(c), cell_literal(c, v)?)))
                .collect::<anyhow::Result<Vec<_>>>()?
                .join(", ");
            statements.push(format!(
                "UPDATE {target} SET {set_clause} WHERE {};",
                where_clause(row)?
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
                    .map(|&(c, v)| cell_literal(c, v))
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .join(", ");
                statements.push(format!("INSERT INTO {target} ({cols}) VALUES ({vals});"));
            }
        }

        for &row in &self.deleted_rows {
            statements.push(format!(
                "DELETE FROM {target} WHERE {};",
                where_clause(row)?
            ));
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
    fn sqlite_binary_edits_roundtrip_null_empty_and_non_utf8() {
        use crate::{DatabaseDriver, SqliteDriver};
        crate::get_runtime().block_on(async {
            let driver = SqliteDriver::new_memory();
            driver
                .execute_query("CREATE TABLE blobs (id INTEGER PRIMARY KEY, data BLOB)")
                .await
                .unwrap();
            driver
                .execute_query("INSERT INTO blobs VALUES (1, X'00ff'), (2, NULL)")
                .await
                .unwrap();
            let (pager, _, _) = driver
                .execute_query("SELECT id, data FROM blobs ORDER BY id")
                .await
                .unwrap();
            let mut edits = RowChanges::default();
            edits
                .changes
                .insert((0, 1), Some(crate::value::hex_text(b"\\xdead\0\xff")));
            edits
                .changes
                .insert((1, 1), Some(crate::value::hex_text(b"")));
            let statements = edits
                .plan_sql("blobs", &pager, &[0], SqlDialect(DbDriverKind::Sqlite))
                .unwrap();
            driver.apply_row_changes(&statements).await.unwrap();
            let (stored, _, _) = driver
                .execute_query("SELECT data, typeof(data) FROM blobs ORDER BY id")
                .await
                .unwrap();
            assert_eq!(stored.get_cell(0, 0).unwrap(), b"\\xdead\0\xff");
            assert_eq!(stored.get_cell(1, 0).unwrap(), b"");
            assert_eq!(stored.get_cell(1, 1).unwrap(), b"blob");
            let mut inserted = RowChanges::default();
            inserted.inserted_rows.push(vec![
                Some("3".into()),
                Some(crate::value::hex_text(b"\\xdead")),
            ]);
            driver
                .apply_row_changes(
                    &inserted
                        .plan_sql("blobs", &pager, &[0], SqlDialect(DbDriverKind::Sqlite))
                        .unwrap(),
                )
                .await
                .unwrap();
            let (all, _, _) = driver
                .execute_query("SELECT id, data FROM blobs ORDER BY id")
                .await
                .unwrap();
            assert_eq!(all.get_cell(2, 1).unwrap(), b"\\xdead");
            let mut deleted = RowChanges::default();
            deleted.deleted_rows.insert(2);
            driver
                .apply_row_changes(
                    &deleted
                        .plan_sql("blobs", &all, &[], SqlDialect(DbDriverKind::Sqlite))
                        .unwrap(),
                )
                .await
                .unwrap();
            let (remaining, _, _) = driver
                .execute_query("SELECT id FROM blobs ORDER BY id")
                .await
                .unwrap();
            assert_eq!(remaining.total_rows(), 2);
            edits.changes.insert((0, 1), Some("invalid hex".into()));
            assert!(
                edits
                    .plan_sql("blobs", &pager, &[0], SqlDialect(DbDriverKind::Sqlite))
                    .is_err()
            );
        });
    }

    #[test]
    fn oracle_empty_binary_is_rejected_instead_of_becoming_null() {
        let pager = QueryResultPager::new(vec![ColumnMeta::new("data", DataType::Bytea)]).unwrap();
        pager.append_row([Some(b"\\x00".as_slice())]).unwrap();
        let mut changes = RowChanges::default();
        changes.changes.insert((0, 0), Some("\\x".into()));
        let error = changes
            .plan_sql("items", &pager, &[], SqlDialect(DbDriverKind::Oracle))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Empty binary edits are unavailable for Oracle")
        );
        changes.changes.insert((0, 0), None);
        assert!(
            changes
                .plan_sql("items", &pager, &[], SqlDialect(DbDriverKind::Oracle))
                .unwrap()[0]
                .contains("SET \"data\" = NULL")
        );
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
            ["UPDATE \"public\".\"users\" SET \"username\" = E'it''s new' WHERE \"id\" = E'42';"]
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
                "INSERT INTO `users` (`username`) VALUES (CONVERT(X'6e65775c75736572' USING utf8mb4));",
                "INSERT INTO `users` () VALUES ();",
                "DELETE FROM `users` WHERE `id` = CONVERT(X'3433' USING utf8mb4) AND `username` IS NULL;",
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
                crate::filter::value_literal(kind, "new")
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
