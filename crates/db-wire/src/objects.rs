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
    /// Signature for overloaded routines, relation name for triggers.
    pub qualifier: String,
}

/// Object pages are ordered by native identity. None means unsupported.
#[derive(Clone, Debug)]
pub struct ObjectPage {
    pub objects: Vec<ObjectSummary>,
    pub next: Option<u32>,
}

pub const OBJECT_PAGE_SIZE: usize = 100;
