//! Editable destination-column mapping shared by CSV import and table transfer.
use db_wire::{
    ColumnMeta,
    transfer::mapping::{ColumnMapping, Conversion, Mapping},
};

pub(super) const CONVERSIONS: [(Conversion, &str); 13] = [
    (Conversion::Auto, "Auto"),
    (Conversion::Text, "Text"),
    (Conversion::Integer, "Integer"),
    (Conversion::Decimal, "Decimal"),
    (Conversion::Float, "Float"),
    (Conversion::Boolean, "Boolean"),
    (Conversion::Date, "Date"),
    (Conversion::Time, "Time"),
    (Conversion::Timestamp, "Timestamp"),
    (Conversion::TimestampTz, "Timestamp TZ"),
    (Conversion::Json, "JSON"),
    (Conversion::Uuid, "UUID"),
    (Conversion::BinaryHex, "Binary (hex)"),
];

pub(super) fn conversion_label(conversion: Conversion) -> &'static str {
    CONVERSIONS
        .iter()
        .find(|(c, _)| *c == conversion)
        .map_or("Auto", |(_, label)| label)
}

#[derive(Clone, Debug)]
pub struct MappingRow {
    pub target: ColumnMeta,
    /// `None` skips the destination column so its native default applies.
    pub source: Option<usize>,
    pub conversion: Conversion,
}

#[derive(Clone, Debug, Default)]
pub struct MappingDraft {
    pub sources: Vec<String>,
    pub rows: Vec<MappingRow>,
}

impl MappingDraft {
    /// Maps each destination column to the first source with the same name, ignoring ASCII case.
    pub fn auto(sources: Vec<String>, targets: &[ColumnMeta]) -> Self {
        let rows = targets
            .iter()
            .map(|target| MappingRow {
                target: target.clone(),
                source: sources
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(&target.name)),
                conversion: Conversion::Auto,
            })
            .collect();
        Self { sources, rows }
    }

    /// Keeps choices for the same source header names; new sources are auto-mapped.
    pub fn with_sources(&self, sources: Vec<String>, targets: &[ColumnMeta]) -> Self {
        let same_targets = self.rows.len() == targets.len()
            && self
                .rows
                .iter()
                .zip(targets)
                .all(|(r, t)| r.target.name == t.name);
        if self.sources == sources && same_targets {
            return self.clone();
        }
        Self::auto(sources, targets)
    }

    pub fn set_source(&mut self, row: usize, source: Option<usize>) {
        if let Some(r) = self.rows.get_mut(row)
            && source.is_none_or(|s| s < self.sources.len())
        {
            r.source = source;
        }
    }

    pub fn set_conversion(&mut self, row: usize, conversion: Conversion) {
        if let Some(r) = self.rows.get_mut(row) {
            r.conversion = conversion;
        }
    }

    pub fn mapped_count(&self) -> usize {
        self.rows.iter().filter(|r| r.source.is_some()).count()
    }

    pub fn build(&self) -> anyhow::Result<Mapping> {
        let columns: Vec<ColumnMapping> = self
            .rows
            .iter()
            .filter_map(|r| {
                r.source.map(|source| ColumnMapping {
                    source,
                    target: r.target.clone(),
                    conversion: r.conversion,
                })
            })
            .collect();
        anyhow::ensure!(
            !columns.is_empty(),
            "Map at least one destination column to a source column"
        );
        Mapping::new(self.sources.len(), columns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::DataType;

    fn targets() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("Name", DataType::Text),
            ColumnMeta::new("note", DataType::Text),
        ]
    }

    #[test]
    fn auto_maps_by_case_insensitive_name_and_skips_unknown() {
        let draft = MappingDraft::auto(vec!["NAME".into(), "ID".into()], &targets());
        let sources: Vec<_> = draft.rows.iter().map(|r| r.source).collect();
        assert_eq!(sources, vec![Some(1), Some(0), None]);
        assert_eq!(draft.mapped_count(), 2);
        let mapping = draft.build().unwrap();
        let names: Vec<_> = mapping
            .columns()
            .iter()
            .map(|c| (c.target.name.as_str(), c.source))
            .collect();
        assert_eq!(names, vec![("id", 1), ("Name", 0)]);
    }

    #[test]
    fn manual_choices_drive_the_built_mapping() {
        let mut draft = MappingDraft::auto(vec!["a".into(), "b".into()], &targets());
        assert!(draft.build().is_err(), "nothing matches by name");
        draft.set_source(2, Some(1));
        draft.set_conversion(2, Conversion::Json);
        draft.set_source(0, Some(7)); // Out of range is ignored.
        assert_eq!(draft.rows[0].source, None);
        let mapping = draft.build().unwrap();
        assert_eq!(mapping.columns().len(), 1);
        assert_eq!(mapping.columns()[0].target.name, "note");
        assert_eq!(mapping.columns()[0].source, 1);
        assert_eq!(mapping.columns()[0].conversion, Conversion::Json);
        draft.set_source(2, None);
        assert!(draft.build().is_err());
    }

    #[test]
    fn same_sources_keep_choices_and_new_sources_remap() {
        let mut draft = MappingDraft::auto(vec!["id".into(), "x".into()], &targets());
        draft.set_source(2, Some(1));
        let kept = draft.with_sources(vec!["id".into(), "x".into()], &targets());
        assert_eq!(kept.rows[2].source, Some(1));
        let remapped = draft.with_sources(vec!["x".into(), "id".into()], &targets());
        assert_eq!(remapped.rows[0].source, Some(1));
        assert_eq!(remapped.rows[2].source, None);
    }

    #[test]
    fn labels_cover_every_conversion() {
        assert_eq!(conversion_label(Conversion::BinaryHex), "Binary (hex)");
        assert_eq!(conversion_label(Conversion::Auto), "Auto");
    }
}
