/// Read-only object families exposed by the catalog explorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Sequence,
    Routine,
    Trigger,
    Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectSummary {
    pub id: u32,
    pub schema: String,
    pub name: String,
    /// Signature for overloaded routines or the owning relation name, when applicable.
    pub qualifier: String,
}

/// Object pages are ordered by native identity. None means unsupported.
#[derive(Clone, Debug)]
pub struct ObjectPage {
    pub objects: Vec<ObjectSummary>,
    pub next: Option<u32>,
}

pub const OBJECT_PAGE_SIZE: usize = 100;

/// Searchable catalog definition families.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionKind {
    Table,
    View,
    MaterializedView,
    Routine,
    Trigger,
    Rule,
    Index,
    Constraint,
    Policy,
    Statistics,
}

#[derive(Clone, Debug)]
pub struct DefinitionHit {
    pub kind: DefinitionKind,
    pub object: ObjectSummary,
    /// Source as searched. None indicates the preview exceeds the size limit.
    pub source: Option<String>,
}

pub const DEFINITION_SEARCH_LIMIT: usize = 50;
pub const DEFINITION_SOURCE_LIMIT: i32 = 64 * 1024;
pub const DEFINITION_INPUT_LIMIT: usize = 4096;

/// A matching value and its provider-specific row preview locator.
#[derive(Clone, Debug)]
pub struct DataSearchHit {
    pub column: String,
    pub preview: String,
    pub locator: DataRowLocator,
}

#[derive(Clone, Debug)]
pub struct DataRowLocator(pub(crate) DataRowIdentity);

#[derive(Clone, Debug)]
pub(crate) enum DataRowIdentity {
    Postgres(PostgresRowLocator),
    Sqlite(DataRowPreview),
    MySql(DataRowPreview),
}

#[derive(Clone, Debug)]
pub(crate) struct PostgresRowLocator {
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) relation: u32,
    pub(crate) file: u32,
    pub(crate) column: String,
    pub(crate) tuple: String,
    pub(crate) version: String,
}

#[derive(Clone, Debug)]
pub struct DataRowPreview {
    pub columns: Vec<(String, Option<String>)>,
    pub omitted_columns: usize,
}

pub const DATA_SEARCH_LIMIT: usize = 50;
pub const DATA_PREVIEW_CHARS: i32 = 512;
