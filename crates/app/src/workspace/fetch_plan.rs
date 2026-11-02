use super::tab_state::script_fetch_query;
use super::*;
use db_wire::sql::{SqlDialect, quote_ident, table_page_suffix};
impl Workspace {
    /// SQL of the first fetch for a tab, as shown in the "SQL" dialog
    pub fn build_query_for_tab(&self, tab_idx: usize) -> String {
        self.build_fetch_query(tab_idx, 0).0
    }

    /// SQL reading one chunk of `fetch.size` rows starting at `offset`, and whether it is
    /// limited by the fetch size (false for raw SQL or a filter with its own LIMIT)
    pub(super) fn build_fetch_query(&self, tab_idx: usize, offset: u64) -> (String, bool) {
        let tab = match self.tabs.get(tab_idx) {
            Some(t) => t,
            None => return ("SELECT 1;".to_string(), false),
        };

        let schema = tab.schema_name.as_deref().unwrap_or("public");
        let table = match tab.table_name.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ => {
                return if self.tab_provider(tab_idx).uses_odbc() {
                    // Execute vendor SQL as written: CTEs, TOP and PL/SQL are not LIMIT dialects.
                    (tab.script_sql.clone().unwrap_or_default(), false)
                } else {
                    script_fetch_query(tab, offset)
                };
            }
        };

        let target = SqlDialect(self.tab_provider(tab_idx)).qualified_table(schema, table);

        let filter = tab.filter_sql.trim();
        let f_upper = filter.to_uppercase();
        let clauses = sql_intel::filter_clauses(filter);
        let has_order = clauses
            .as_ref()
            .map(|c| c.order_by)
            .unwrap_or_else(|| f_upper.contains("ORDER BY"));
        let has_limit = clauses
            .as_ref()
            .map(|c| c.limit || c.offset || c.fetch)
            .unwrap_or_else(|| {
                ["LIMIT", "OFFSET", "FETCH"]
                    .iter()
                    .any(|word| f_upper.split_whitespace().any(|part| part == *word))
            });

        // A filter that orders or limits its rows itself already ends the statement,
        // so the header ordering cannot be appended after it
        let order_suffix = match &tab.grid.ordering {
            Some(o) if !has_order && !has_limit => {
                format!(
                    " ORDER BY {} {}",
                    quote_ident(self.tab_provider(tab_idx), &o.column),
                    if o.descending { "DESC" } else { "ASC" }
                )
            }
            _ => String::new(),
        };

        let limit_suffix = table_page_suffix(
            self.tab_provider(tab_idx),
            tab.fetch.size,
            offset,
            !order_suffix.is_empty() || has_order,
        );

        if filter.is_empty() {
            (
                format!("SELECT * FROM {}{}{};", target, order_suffix, limit_suffix),
                true,
            )
        } else {
            // A filter with its own LIMIT decides the row count, so it is not fetched in chunks
            let paged = !has_limit;
            let limit_suffix = if paged { limit_suffix.as_str() } else { "" };

            let sql = if f_upper.starts_with("WHERE")
                || f_upper.starts_with("ORDER BY")
                || f_upper.starts_with("GROUP BY")
                || f_upper.starts_with("HAVING")
                || f_upper.starts_with("LIMIT")
                || f_upper.starts_with("OFFSET")
                || f_upper.starts_with("FETCH")
            {
                format!(
                    "SELECT * FROM {} {}{}{};",
                    target, filter, order_suffix, limit_suffix
                )
            } else {
                format!(
                    "SELECT * FROM {} WHERE {}{}{};",
                    target, filter, order_suffix, limit_suffix
                )
            };
            (sql, paged)
        }
    }
}

#[cfg(test)]
mod provider_tests {
    use super::{quote_ident, table_page_suffix};
    use db_wire::DbDriverKind;

    #[test]
    fn enterprise_table_paging_uses_vendor_syntax() {
        assert_eq!(
            table_page_suffix(DbDriverKind::SqlServer, 50, 100, false),
            " ORDER BY (SELECT NULL) OFFSET 100 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        assert_eq!(
            table_page_suffix(DbDriverKind::SqlServer, 50, 0, true),
            " OFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        for kind in [DbDriverKind::Oracle, DbDriverKind::Db2] {
            assert_eq!(
                table_page_suffix(kind, 50, 100, false),
                " OFFSET 100 ROWS FETCH NEXT 50 ROWS ONLY"
            );
            assert_eq!(quote_ident(kind, "a\"b"), "\"a\"\"b\"");
        }
    }

    #[test]
    fn mariadb_uses_mysql_identifier_quoting() {
        assert_eq!(
            quote_ident(DbDriverKind::MariaDb, "user`name"),
            "`user``name`"
        );
    }
}
