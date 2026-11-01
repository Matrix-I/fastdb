use db_wire::{ColumnMeta, DbDriverKind, sql::generate_table_ddl};
#[derive(Clone)]
pub(in crate::workspace) struct TableProperties {
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnMeta>,
    pub constraints: Vec<(String, String)>,
    pub indexes: Vec<String>,
    pub ddl: String,
}
impl TableProperties {
    pub fn new(
        kind: DbDriverKind,
        schema: String,
        table: String,
        columns: Vec<ColumnMeta>,
        constraints: Vec<(String, String)>,
        indexes: Vec<String>,
    ) -> Self {
        let ddl = generate_table_ddl(kind, &schema, &table, &columns, &constraints, &indexes);
        Self {
            schema,
            table,
            columns,
            constraints,
            indexes,
            ddl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TableProperties;
    use db_wire::{ColumnMeta, DataType, DbDriverKind};
    #[test]
    fn unavailable_metadata_does_not_create_sample_columns_or_fake_definitions() {
        let empty = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "additionalInfo".into(),
            vec![],
            vec![],
            vec![],
        );
        assert!(empty.columns.is_empty());
        assert!(empty.indexes.is_empty());
        assert!(empty.constraints.is_empty());
        assert!(empty.ddl.contains("unavailable"));
        assert!(!empty.ddl.contains("CREATE TABLE"));
        let known = TableProperties::new(
            DbDriverKind::Postgres,
            "other".into(),
            "additionalInfo".into(),
            vec![ColumnMeta::new("real_column", DataType::Text).with_raw_type("text")],
            vec![],
            vec![],
        );
        assert!(known.ddl.contains("real_column"));
        assert!(!known.ddl.contains("ubiid"));
        assert!(empty.columns.is_empty());
    }
}
