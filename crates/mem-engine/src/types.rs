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
    pub raw_type: Option<String>,
    pub default_val: Option<String>,
    pub collation: Option<String>,
    pub comment: Option<String>,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
            is_primary_key: false,
            is_foreign_key: false,
            raw_type: None,
            default_val: None,
            collation: None,
            comment: None,
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

    pub fn with_raw_type(mut self, raw_type: impl Into<String>) -> Self {
        self.raw_type = Some(raw_type.into());
        self
    }

    pub fn with_default(mut self, default_val: impl Into<String>) -> Self {
        self.default_val = Some(default_val.into());
        self
    }

    pub fn with_collation(mut self, collation: impl Into<String>) -> Self {
        self.collation = Some(collation.into());
        self
    }

    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    pub fn display_type(&self) -> String {
        if let Some(ref raw) = self.raw_type {
            let r = raw.trim().to_lowercase();
            if r == "character varying" {
                "varchar".to_string()
            } else {
                r
            }
        } else {
            match self.data_type {
                DataType::Varchar => "varchar".to_string(),
                DataType::Json => "jsonb".to_string(),
                DataType::Text => "text".to_string(),
                DataType::Int4 => "int4".to_string(),
                DataType::Int8 => "int8".to_string(),
                DataType::Int2 => "int2".to_string(),
                DataType::Bool => "bool".to_string(),
                DataType::Float4 => "float4".to_string(),
                DataType::Float8 => "float8".to_string(),
                DataType::Numeric => "numeric".to_string(),
                DataType::Date => "date".to_string(),
                DataType::Time => "time".to_string(),
                DataType::Timestamp => "timestamp".to_string(),
                DataType::TimestampTz => "timestamptz".to_string(),
                DataType::Uuid => "uuid".to_string(),
                DataType::Bytea => "bytea".to_string(),
                DataType::Unknown => "unknown".to_string(),
            }
        }
    }
}
