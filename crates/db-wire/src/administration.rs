//! Provider-neutral administration contracts and PostgreSQL SQL templates.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminFamily {
    Routines,
    Sequences,
    Triggers,
    Rules,
    Policies,
    Partitions,
    Roles,
    Privileges,
}

impl AdminFamily {
    pub const ALL: [Self; 8] = [
        Self::Routines,
        Self::Sequences,
        Self::Triggers,
        Self::Rules,
        Self::Policies,
        Self::Partitions,
        Self::Roles,
        Self::Privileges,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Routines => "Routines",
            Self::Sequences => "Sequences",
            Self::Triggers => "Triggers",
            Self::Rules => "Rules",
            Self::Policies => "Policies",
            Self::Partitions => "Partitions",
            Self::Roles => "Roles",
            Self::Privileges => "Privileges",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdminObject {
    pub id: u32,
    pub schema: String,
    pub name: String,
    /// Quoted owning relation, or partition parent; empty when inapplicable.
    pub parent: String,
    pub details: String,
    pub source: Option<String>,
    /// Native overload-qualified invocation with editable, typed NULL arguments.
    pub invocation: Option<String>,
}

pub struct AdminPage {
    pub objects: Vec<AdminObject>,
    pub next: Option<u32>,
}

#[derive(Default, Debug)]
pub struct AdminOutcome {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub affected: u64,
    pub truncated: bool,
}

pub const ADMIN_PAGE_SIZE: usize = 50;
pub const ADMIN_SQL_LIMIT: usize = 256 * 1024;
pub const ADMIN_RESULT_ROWS: usize = 100;
pub const ADMIN_RESULT_COLUMNS: usize = 32;
pub const ADMIN_CELL_CHARS: usize = 512;

/// Templates contain intentionally editable SQL expressions. Only catalog-derived
/// identifiers are interpolated; free-form user SQL is reviewed before execution.
pub fn postgres_templates(
    family: AdminFamily,
    object: Option<&AdminObject>,
) -> Vec<(&'static str, String)> {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let name = object
        .map(|o| quote(&o.name))
        .unwrap_or_else(|| "\"new_object\"".into());
    let target = object
        .map(|o| format!("{}.{}", quote(&o.schema), name))
        .unwrap_or_else(|| "\"public\".\"new_object\"".into());
    let parent = object
        .filter(|o| !o.parent.is_empty())
        .map(|o| o.parent.clone())
        .unwrap_or_else(|| "\"public\".\"target_table\"".into());
    let mut templates = match family {
        AdminFamily::Routines => vec![("New function", "CREATE OR REPLACE FUNCTION \"public\".\"new_function\"(value integer) RETURNS integer LANGUAGE sql AS $body$ SELECT value + 1 $body$;".into()),
            ("New procedure", "CREATE OR REPLACE PROCEDURE \"public\".\"new_procedure\"(value integer) LANGUAGE plpgsql AS $body$ BEGIN RAISE NOTICE 'value: %', value; END $body$;".into())],
        AdminFamily::Sequences => vec![("Create sequence", "CREATE SEQUENCE \"public\".\"new_sequence\" START WITH 1;".into()),
            ("Restart sequence", format!("ALTER SEQUENCE {target} RESTART WITH 1;")),
            ("Alter sequence", format!("ALTER SEQUENCE {target} INCREMENT BY 1 CACHE 1 NO CYCLE;")),
            ("Drop sequence", format!("DROP SEQUENCE {target} RESTRICT;"))],
        AdminFamily::Triggers => vec![("Create trigger", format!("CREATE OR REPLACE TRIGGER \"new_trigger\" BEFORE INSERT ON {parent} FOR EACH ROW EXECUTE FUNCTION \"public\".\"trigger_function\"();")),
            ("Enable trigger", format!("ALTER TABLE {parent} ENABLE TRIGGER {name};")),
            ("Disable trigger", format!("ALTER TABLE {parent} DISABLE TRIGGER {name};")),
            ("Drop trigger", format!("DROP TRIGGER {name} ON {parent} RESTRICT;"))],
        AdminFamily::Rules => vec![("Create rule", format!("CREATE OR REPLACE RULE \"new_rule\" AS ON DELETE TO {parent} DO INSTEAD NOTHING;")),
            ("Enable rule", format!("ALTER TABLE {parent} ENABLE RULE {name};")),
            ("Disable rule", format!("ALTER TABLE {parent} DISABLE RULE {name};")),
            ("Drop rule", format!("DROP RULE {name} ON {parent} RESTRICT;"))],
        AdminFamily::Policies => vec![("Create policy", format!("CREATE POLICY \"new_policy\" ON {parent} FOR SELECT TO PUBLIC USING (true);")),
            ("Enable RLS", format!("ALTER TABLE {parent} ENABLE ROW LEVEL SECURITY;")),
            ("Disable RLS", format!("ALTER TABLE {parent} DISABLE ROW LEVEL SECURITY;")),
            ("Drop policy", format!("DROP POLICY {name} ON {parent};"))],
        AdminFamily::Partitions => {
            let root = object.filter(|o| o.parent.is_empty()).map(|_| target.clone()).unwrap_or(parent);
            vec![("Create partition", format!("CREATE TABLE \"public\".\"new_partition\" PARTITION OF {root} FOR VALUES FROM (0) TO (100);")),
                ("Attach partition", format!("ALTER TABLE {root} ATTACH PARTITION \"public\".\"existing_table\" FOR VALUES FROM (0) TO (100);")),
                ("Detach partition", format!("ALTER TABLE {root} DETACH PARTITION {target};"))]
        }
        AdminFamily::Roles => vec![("Create role", "CREATE ROLE \"new_role\" NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;".into()),
            ("Create user", "CREATE ROLE \"new_user\" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;".into()),
            ("Allow login", format!("ALTER ROLE {name} LOGIN;")),
            ("Disable login", format!("ALTER ROLE {name} NOLOGIN;")),
            ("Grant membership", format!("GRANT {name} TO \"member_role\";")),
            ("Revoke membership", format!("REVOKE {name} FROM \"member_role\";")),
            ("Drop role", format!("DROP ROLE {name};"))],
        AdminFamily::Privileges => vec![("Grant privilege", format!("GRANT SELECT ON TABLE {target} TO \"recipient_role\";")),
            ("Revoke privilege", format!("REVOKE SELECT ON TABLE {target} FROM \"recipient_role\" RESTRICT;")),
            ("Grant schema usage", "GRANT USAGE ON SCHEMA \"public\" TO \"recipient_role\";".into()),
            ("Grant routine execute", "GRANT EXECUTE ON FUNCTION \"public\".\"function_name\"(integer) TO \"recipient_role\";".into()),
            ("Grant sequence usage", "GRANT USAGE ON SEQUENCE \"public\".\"sequence_name\" TO \"recipient_role\";".into())],
    };
    if let Some(source) = object.and_then(|o| o.source.clone()) {
        templates.insert(0, ("Edit source", source));
    }
    if let Some(sql) = object.and_then(|o| o.invocation.clone()) {
        templates.push(("Run with arguments", sql));
    }
    templates
}

/// PostgreSQL validates the complete statement with Parse before execution.
/// A narrow first-keyword allowlist excludes transaction/session commands even
/// when comments precede them. Comments may instead follow the first keyword.
pub fn validate_admin_sql(sql: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !sql.contains('\0') && sql.len() <= ADMIN_SQL_LIMIT,
        "SQL must contain no NUL and be at most 256 KiB"
    );
    let first = sql
        .trim_start()
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    anyhow::ensure!(
        matches!(
            first.as_str(),
            "CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE" | "COMMENT" | "SELECT" | "CALL"
        ),
        "Start with CREATE, ALTER, DROP, GRANT, REVOKE, COMMENT, SELECT or CALL; execute one statement at a time"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates_quote_catalog_identifiers_and_preserve_native_invocation() {
        let object = AdminObject {
            id: 1,
            schema: "odd\"schema".into(),
            name: "x\"; DROP ROLE y; --".into(),
            parent: "\"s\".\"table\"".into(),
            details: String::new(),
            source: None,
            invocation: Some("SELECT \"s\".\"f\"(NULL::integer);".into()),
        };
        let sequence = postgres_templates(AdminFamily::Sequences, Some(&object));
        assert_eq!(
            sequence[1].1,
            "ALTER SEQUENCE \"odd\"\"schema\".\"x\"\"; DROP ROLE y; --\" RESTART WITH 1;"
        );
        assert_eq!(sequence.last().unwrap().1, object.invocation.unwrap());
        assert!(
            postgres_templates(
                AdminFamily::Triggers,
                Some(&AdminObject {
                    invocation: None,
                    ..object
                })
            )
            .iter()
            .any(|(_, sql)| sql.starts_with("ALTER TABLE \"s\".\"table\" DISABLE TRIGGER"))
        );
    }
    #[test]
    fn policy_edit_uses_the_existing_predicate_and_roles() {
        let sql = "ALTER POLICY tenant ON app.documents TO reader USING (tenant_id = 1);";
        let object = AdminObject {
            id: 1,
            schema: "app".into(),
            name: "tenant".into(),
            parent: "app.documents".into(),
            details: String::new(),
            source: Some(sql.into()),
            invocation: None,
        };
        let edits = postgres_templates(AdminFamily::Policies, Some(&object))
            .into_iter()
            .filter(|(_, sql)| sql.starts_with("ALTER POLICY"))
            .collect::<Vec<_>>();
        assert_eq!(edits, vec![("Edit source", sql.into())]);
    }
    #[test]
    fn transaction_controls_and_oversized_sql_are_rejected() {
        for sql in [
            "COMMIT",
            "ROLLBACK",
            "SET statement_timeout=0",
            "/* comment */ COMMIT",
            "-- comment\nEND",
            "",
            "SELECT '\0'",
        ] {
            assert!(validate_admin_sql(sql).is_err(), "{sql:?}");
        }
        assert!(validate_admin_sql(&format!("SELECT '{}'", "x".repeat(ADMIN_SQL_LIMIT))).is_err());
        assert!(
            validate_admin_sql(
                "CREATE FUNCTION f() RETURNS void LANGUAGE plpgsql AS $$ BEGIN NULL; END $$;"
            )
            .is_ok()
        );
    }
}
