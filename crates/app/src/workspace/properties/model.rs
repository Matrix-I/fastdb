use crate::workspace::metadata_state::{MetadataState, MetadataStates};
use db_wire::metadata::MetadataFeature;
use db_wire::{ColumnMeta, DbDriverKind, sql::generate_table_ddl};
use std::sync::Arc;
#[derive(Clone)]
pub(in crate::workspace) struct TableProperties {
    pub(super) kind: DbDriverKind,
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnMeta>,
    pub constraints: Vec<(String, String)>,
    pub indexes: Vec<String>,
    pub ddl: String,
    pub details: Option<db_wire::TableDetails>,
    pub metadata_error: Option<String>,
    pub states: Arc<MetadataStates>,
    pub foreign_keys: Vec<db_wire::ForeignKey>,
}
impl TableProperties {
    pub fn statistics_rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![
            ("Table Name", self.table.clone()),
            ("Schema", self.schema.clone()),
            ("Columns Count", self.columns.len().to_string()),
        ];
        let d = self.details.as_ref();
        let text = |value: Option<&String>| value.cloned().unwrap_or_else(|| "Unavailable".into());
        let flag = |value: Option<bool>| {
            match value {
                Some(true) => "Yes",
                Some(false) => "No",
                None => "Unavailable",
            }
            .to_string()
        };
        let number = |value: Option<i64>| {
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "Unavailable".into())
        };
        rows.extend([
            ("Object ID", text(d.and_then(|d| d.object_id.as_ref()))),
            ("Owner", text(d.and_then(|d| d.owner.as_ref()))),
            ("Tablespace", text(d.and_then(|d| d.tablespace.as_ref()))),
            (
                "Comment",
                d.and_then(|d| {
                    d.comment
                        .clone()
                        .or_else(|| (self.kind == DbDriverKind::Postgres).then(|| "None".into()))
                })
                .unwrap_or_else(|| "Unavailable".into()),
            ),
            (
                "Has Primary Key",
                if let Some(d) = d.filter(|_| self.kind == DbDriverKind::Postgres) {
                    flag(Some(d.constraints.iter().any(|c| c.kind == "PRIMARY KEY")))
                } else if self.columns.is_empty() {
                    "Unavailable".into()
                } else {
                    flag(Some(self.columns.iter().any(|c| c.is_primary_key)))
                },
            ),
            ("Row-Level Security", flag(d.and_then(|d| d.row_security))),
            (
                "Force Row-Level Security",
                flag(d.and_then(|d| d.force_row_security)),
            ),
            ("Partitioned", flag(d.and_then(|d| d.partitioned))),
            (
                "Partition Key",
                text(d.and_then(|d| d.partition_key.as_ref())),
            ),
            ("Persistence", text(d.and_then(|d| d.persistence.as_ref()))),
            (
                if self.kind == DbDriverKind::Sqlite {
                    "Stored Row Estimate (may be stale)"
                } else {
                    "Estimated Rows (ANALYZE)"
                },
                number(d.and_then(|d| d.estimated_rows)),
            ),
            (
                "Table + TOAST (bytes)",
                number(d.and_then(|d| d.table_bytes)),
            ),
            ("Indexes (bytes)", number(d.and_then(|d| d.index_bytes))),
            (
                "Total Relation (bytes)",
                number(d.and_then(|d| d.total_bytes)),
            ),
            ("Last Vacuum", text(d.and_then(|d| d.last_vacuum.as_ref()))),
            (
                "Last Autovacuum",
                text(d.and_then(|d| d.last_autovacuum.as_ref())),
            ),
            (
                "Last Analyze",
                text(d.and_then(|d| d.last_analyze.as_ref())),
            ),
            (
                "Last Autoanalyze",
                text(d.and_then(|d| d.last_autoanalyze.as_ref())),
            ),
        ]);
        if let Some(error) = &self.metadata_error {
            rows.push(("Metadata Error", error.clone()));
        }
        rows
    }

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
            kind,
            schema,
            table,
            columns,
            constraints,
            indexes,
            ddl,
            details: None,
            metadata_error: None,
            states: Arc::default(),
            foreign_keys: Vec::new(),
        }
    }

    pub fn with_states(
        mut self,
        states: Arc<MetadataStates>,
        foreign_keys: Vec<db_wire::ForeignKey>,
    ) -> Self {
        self.states = states;
        self.foreign_keys = foreign_keys;
        self
    }
    pub fn ddl_notice(&self) -> Option<String> {
        if self
            .details
            .as_ref()
            .and_then(|details| details.ddl.as_ref())
            .is_some()
        {
            None
        } else {
            self.section_notice(MetadataFeature::Columns)
        }
    }
    pub(super) fn constraint_details(&self) -> Option<&db_wire::TableDetails> {
        self.details
            .as_ref()
            .filter(|_| self.kind == DbDriverKind::Postgres)
    }

    pub(in crate::workspace) fn statistics_notice(&self) -> Option<String> {
        if self.kind == DbDriverKind::Sqlite {
            if let Some(message) = self
                .states
                .get(&MetadataFeature::TableDetails)
                .and_then(MetadataState::message)
            {
                return Some(message);
            }
            self.details
                .as_ref()
                .and_then(|details| details.estimated_rows)
                .is_none()
                .then(|| "No stored SQLite table row estimate is available.".into())
        } else {
            self.section_notice(MetadataFeature::TableDetails)
        }
    }

    pub fn section_notice(&self, feature: MetadataFeature) -> Option<String> {
        if self.details.is_some()
            && (feature == MetadataFeature::Indexes
                || (self.kind == DbDriverKind::Postgres
                    && matches!(
                        feature,
                        MetadataFeature::Constraints | MetadataFeature::ForeignKeys
                    )))
        {
            return None;
        }
        match self.states.get(&feature) {
            Some(MetadataState::Ready)
                if feature == MetadataFeature::TableDetails && self.details.is_none() =>
            {
                Some("Native details are unavailable for this relation.".into())
            }
            Some(MetadataState::Ready) => None,
            Some(state) => state.message(),
            None => Some("Metadata has not been loaded. Connect and refresh to retry.".into()),
        }
    }
    pub fn with_metadata(
        mut self,
        details: Option<db_wire::TableDetails>,
        error: Option<String>,
    ) -> Self {
        if let Some(ddl) = details.as_ref().and_then(|d| d.ddl.as_ref()) {
            self.ddl = ddl.clone();
        } else if let Some(details) = &details {
            self.ddl = self
                .ddl
                .lines()
                .filter(|line| {
                    !line.starts_with("-- Constraint metadata:")
                        && !line.starts_with("-- Index metadata:")
                })
                .map(|line| format!("{line}\n"))
                .collect();
            for constraint in &details.constraints {
                self.ddl.push_str(&format!(
                    "-- Catalog constraint {}: {}\n",
                    constraint.name.replace(['\r', '\n'], " "),
                    constraint.definition.replace(['\r', '\n'], " ")
                ));
            }
            for index in &details.indexes {
                self.ddl.push_str(&format!(
                    "-- Catalog index: {}\n",
                    index.definition.replace(['\r', '\n'], " ")
                ));
            }
        }
        self.details = details;
        self.metadata_error = error;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::TableProperties;
    use db_wire::{ColumnMeta, DataType, DbDriverKind};

    #[test]
    fn sqlite_index_details_do_not_replace_constraint_availability_or_primary_key() {
        use db_wire::metadata::{MetadataFeature, MetadataSupport};
        let model = TableProperties::new(
            DbDriverKind::Sqlite,
            "main".into(),
            "items".into(),
            vec![ColumnMeta::new("id", DataType::Int4).with_pk(true)],
            vec![],
            vec![],
        )
        .with_metadata(Some(db_wire::TableDetails::default()), None)
        .with_states(
            std::sync::Arc::new(std::collections::HashMap::from([
                (
                    MetadataFeature::Constraints,
                    super::MetadataState::Unavailable(MetadataSupport::NotImplemented),
                ),
                (MetadataFeature::TableDetails, super::MetadataState::Ready),
            ])),
            vec![],
        );
        assert!(model.section_notice(MetadataFeature::Constraints).is_some());
        assert_eq!(model.section_notice(MetadataFeature::Indexes), None);
        assert!(model.constraint_details().is_none());
        assert_eq!(model.section_notice(MetadataFeature::TableDetails), None);
        assert_eq!(
            model.statistics_notice().as_deref(),
            Some("No stored SQLite table row estimate is available.")
        );
        let rows = model.statistics_rows();
        assert!(rows.contains(&("Has Primary Key", "Yes".into())));
        assert!(rows.contains(&("Comment", "Unavailable".into())));
        let analyzed = model.with_metadata(
            Some(db_wire::TableDetails {
                estimated_rows: Some(3),
                ..Default::default()
            }),
            None,
        );
        assert_eq!(analyzed.statistics_notice(), None);
        assert!(
            analyzed
                .statistics_rows()
                .contains(&("Stored Row Estimate (may be stale)", "3".into()))
        );
    }

    #[test]
    fn native_details_remain_available_when_summary_readers_fail() {
        use crate::workspace::metadata_state::MetadataState;
        use db_wire::metadata::MetadataFeature as F;
        use std::sync::Arc;
        let states = Arc::new(
            [F::Columns, F::Constraints, F::Indexes, F::ForeignKeys]
                .into_iter()
                .map(|f| (f, MetadataState::Failed("summary failure".into())))
                .collect(),
        );
        let model = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "items".into(),
            vec![],
            vec![],
            vec![],
        )
        .with_metadata(
            Some(db_wire::TableDetails {
                ddl: Some("CREATE TABLE items ();".into()),
                ..Default::default()
            }),
            None,
        )
        .with_states(states, vec![]);
        for feature in [F::Constraints, F::Indexes, F::ForeignKeys] {
            assert_eq!(model.section_notice(feature), None);
        }
        assert_eq!(model.ddl_notice(), None);
        assert!(
            model
                .section_notice(F::Columns)
                .unwrap()
                .contains("summary failure")
        );
    }
    #[test]
    fn section_notices_preserve_loaded_empty_unavailable_and_failed_states() {
        use crate::workspace::metadata_state::MetadataState;
        use db_wire::metadata::{MetadataFeature as F, MetadataSupport};
        use std::sync::Arc;
        let model = TableProperties::new(
            DbDriverKind::Sqlite,
            "main".into(),
            "empty".into(),
            vec![],
            vec![],
            vec![],
        );
        assert!(
            model
                .section_notice(F::Columns)
                .unwrap()
                .contains("not been loaded")
        );
        let model = model.with_states(
            Arc::new(std::collections::HashMap::from([
                (F::Columns, MetadataState::Ready),
                (F::Indexes, MetadataState::Failed("index fixture".into())),
                (
                    F::Constraints,
                    MetadataState::Unavailable(MetadataSupport::NotImplemented),
                ),
                (
                    F::ForeignKeys,
                    MetadataState::PermissionDenied("native error".into()),
                ),
                (F::TableDetails, MetadataState::Ready),
            ])),
            vec![],
        );
        assert_eq!(model.section_notice(F::Columns), None);
        assert!(
            model
                .section_notice(F::Indexes)
                .unwrap()
                .contains("index fixture")
        );
        assert_eq!(
            model.section_notice(F::Constraints).as_deref(),
            MetadataSupport::NotImplemented.explanation()
        );
        assert!(
            model
                .section_notice(F::ForeignKeys)
                .unwrap()
                .starts_with("Permission denied.")
        );
        assert!(
            model
                .section_notice(F::TableDetails)
                .unwrap()
                .contains("this relation")
        );
    }
    #[test]
    fn ddl_preview_includes_available_definitions_as_safe_comments() {
        let model = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "items".into(),
            vec![ColumnMeta::new("id", DataType::Int4).with_raw_type("integer")],
            vec![("positive".into(), "CHECK".into())],
            vec!["idx".into()],
        )
        .with_metadata(
            Some(db_wire::TableDetails {
                constraints: vec![db_wire::ConstraintDetails {
                    name: "positive\nname".into(),
                    kind: "CHECK".into(),
                    columns: vec!["id".into()],
                    definition: "CHECK (id > 0)\nNOT VALID".into(),
                    validated: false,
                }],
                indexes: vec![db_wire::IndexDetails {
                    name: "idx".into(),
                    method: "btree".into(),
                    columns: vec!["id".into()],
                    unique: false,
                    valid: true,
                    definition: "CREATE INDEX idx\nON public.items (id)".into(),
                }],
                ..Default::default()
            }),
            None,
        );
        assert!(!model.ddl.contains("definition unavailable"));
        assert!(
            model
                .ddl
                .contains("-- Catalog constraint positive name: CHECK (id > 0) NOT VALID")
        );
        assert!(
            model
                .ddl
                .contains("-- Catalog index: CREATE INDEX idx ON public.items (id)")
        );
        assert!(!model.ddl.lines().any(|line| line.starts_with("ON public")));
    }

    #[test]
    fn catalog_ddl_replaces_the_partial_preview_verbatim() {
        let ddl = "CREATE TABLE \"schema\".\"table\" ();\n-- catalog output\n";
        let model = TableProperties::new(
            DbDriverKind::Postgres,
            "schema".into(),
            "table".into(),
            vec![],
            vec![],
            vec![],
        )
        .with_metadata(
            Some(db_wire::TableDetails {
                ddl: Some(ddl.into()),
                ..Default::default()
            }),
            None,
        );
        assert_eq!(model.ddl, ddl);
    }

    #[test]
    fn statistics_preserve_unknown_values_and_show_metadata_errors() {
        let model = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "items".into(),
            vec![],
            vec![],
            vec![],
        );
        let values: std::collections::HashMap<_, _> = model.statistics_rows().into_iter().collect();
        assert_eq!(values["Estimated Rows (ANALYZE)"], "Unavailable");
        assert_eq!(values["Row-Level Security"], "Unavailable");
        assert_eq!(values["Has Primary Key"], "Unavailable");
        let model = model.with_metadata(
            Some(db_wire::TableDetails {
                estimated_rows: Some(0),
                row_security: Some(false),
                table_bytes: Some(0),
                ..Default::default()
            }),
            None,
        );
        let values: std::collections::HashMap<_, _> = model.statistics_rows().into_iter().collect();
        assert_eq!(values["Estimated Rows (ANALYZE)"], "0");
        assert_eq!(values["Row-Level Security"], "No");
        assert_eq!(values["Table + TOAST (bytes)"], "0");
        assert_eq!(values["Last Analyze"], "Unavailable");
        let failed_refresh = model.with_metadata(None, Some("Permission denied".into()));
        let values: std::collections::HashMap<_, _> =
            failed_refresh.statistics_rows().into_iter().collect();
        assert_eq!(values["Estimated Rows (ANALYZE)"], "Unavailable");
        assert_eq!(values["Metadata Error"], "Permission denied");
    }
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
