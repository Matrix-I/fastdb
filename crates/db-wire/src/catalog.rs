//! Optional, read-only provider catalog sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogSection {
    Dependencies,
    References,
    Partitions,
    Triggers,
    Rules,
    Policies,
    Permissions,
}
impl CatalogSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dependencies => "Dependencies",
            Self::References => "References",
            Self::Partitions => "Partitions",
            Self::Triggers => "Triggers",
            Self::Rules => "Rules",
            Self::Policies => "Policies",
            Self::Permissions => "Permissions",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    pub name: String,
    pub kind: String,
    pub definition: String,
    pub relation: Option<(String, String)>,
}
