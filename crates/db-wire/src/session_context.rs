//! Provider-specific context discovery and schema changes for SQL tab sessions.
use crate::DbDriverKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKind {
    Database,
    Schema,
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        return Err("Context names must contain 1–1024 bytes without control characters".into());
    }
    Ok(())
}

/// Bounded catalog results. An extra row lets the caller report truncation explicitly.
pub fn choices_sql(provider: DbDriverKind, context: ContextKind) -> Result<&'static str, String> {
    use ContextKind::*;
    use DbDriverKind::*;
    match (provider, context) {
        (Postgres, Database) => Ok(
            "SELECT datname FROM pg_catalog.pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname LIMIT 65",
        ),
        (Postgres, Schema) => Ok(
            "SELECT nspname FROM pg_catalog.pg_namespace WHERE has_schema_privilege(oid, 'USAGE') ORDER BY nspname LIMIT 65",
        ),
        (MySql | MariaDb, Database) => {
            Ok("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME LIMIT 65")
        }
        (SqlServer, Database) => Ok(
            "SELECT TOP (65) name FROM sys.databases WHERE state = 0 AND HAS_DBACCESS(name) = 1 ORDER BY name",
        ),
        (Oracle, Schema) => {
            Ok("SELECT USERNAME FROM ALL_USERS ORDER BY USERNAME FETCH FIRST 65 ROWS ONLY")
        }
        (Db2, Schema) => Ok(
            "SELECT SCHEMANAME FROM SYSCAT.SCHEMATA ORDER BY SCHEMANAME FETCH FIRST 65 ROWS ONLY",
        ),
        _ => Err(
            "This provider does not support this context selector; use a separate saved connection"
                .into(),
        ),
    }
}

/// Names are quoted identifiers, never SQL fragments or string values.
pub fn schema_statements(provider: DbDriverKind, name: &str) -> Result<[String; 2], String> {
    validate_name(name)?;
    if provider == DbDriverKind::Postgres && name == "$user" {
        return Err("PostgreSQL reserves $user in search_path".into());
    }
    let name = crate::sql::quote_ident(provider, name);
    use DbDriverKind::*;
    Ok(match provider {
        Postgres => [
            format!("SET search_path TO {name}"),
            "SELECT current_schema()".into(),
        ],
        Oracle => [
            format!("ALTER SESSION SET CURRENT_SCHEMA = {name}"),
            "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL".into(),
        ],
        Db2 => [
            format!("SET CURRENT SCHEMA = {name}"),
            "VALUES CURRENT SCHEMA".into(),
        ],
        _ => return Err("This provider has no mutable session default schema".into()),
    })
}
