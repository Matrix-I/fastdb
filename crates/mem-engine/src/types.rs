use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric,
    Text,
    Varchar,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Uuid,
    Json,
    Bytea,
    Unknown,
}

impl DataType {
    pub fn display_name(&self) -> &'static str {
        match self {
            DataType::Bool => "BOOL",
            DataType::Int2 => "INT2",
            DataType::Int4 => "INT4",
            DataType::Int8 => "INT8",
            DataType::Float4 => "FLOAT4",
            DataType::Float8 => "FLOAT8",
            DataType::Numeric => "NUMERIC",
            DataType::Text => "TEXT",
            DataType::Varchar => "VARCHAR",
            DataType::Date => "DATE",
            DataType::Time => "TIME",
            DataType::Timestamp => "TIMESTAMP",
            DataType::TimestampTz => "TIMESTAMPTZ",
            DataType::Uuid => "UUID",
            DataType::Json => "JSON",
            DataType::Bytea => "BYTEA",
            DataType::Unknown => "UNKNOWN",
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
            is_primary_key: false,
            is_foreign_key: false,
        }
    }

    pub fn with_pk(mut self, is_pk: bool) -> Self {
        self.is_primary_key = is_pk;
        self
    }

    pub fn with_fk(mut self, is_fk: bool) -> Self {
        self.is_foreign_key = is_fk;
        self
    }

    pub fn with_nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }
}
