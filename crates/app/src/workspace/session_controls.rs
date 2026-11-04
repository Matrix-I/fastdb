use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IsolationLevel {
    ReadUncommitted,
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

#[derive(Clone, Copy)]
enum SessionSetting {
    Isolation(IsolationLevel),
    AutoCommit(bool),
}

impl IsolationLevel {
    fn label(self) -> &'static str {
        match self {
            Self::ReadUncommitted => "Read uncommitted",
            Self::ReadCommitted => "Read committed",
            Self::RepeatableRead => "Repeatable read",
            Self::Serializable => "Serializable",
        }
    }
}

fn isolation_levels(provider: DbDriverKind) -> &'static [IsolationLevel] {
    use IsolationLevel::*;
    match provider {
        DbDriverKind::Postgres => &[ReadCommitted, RepeatableRead, Serializable],
        DbDriverKind::MySql | DbDriverKind::MariaDb => {
            &[ReadUncommitted, ReadCommitted, RepeatableRead, Serializable]
        }
        DbDriverKind::Sqlite => &[Serializable],
        _ => &[],
    }
}

fn isolation_statements(
    provider: DbDriverKind,
    level: IsolationLevel,
) -> Result<[String; 2], String> {
    if !isolation_levels(provider).contains(&level) {
        return Err("This isolation level is not supported by this provider".into());
    }
    let level = level.label().to_ascii_uppercase();
    Ok(match provider {
        DbDriverKind::Postgres => [
            format!("SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL {level}"),
            "SHOW transaction_isolation".into(),
        ],
        DbDriverKind::MySql | DbDriverKind::MariaDb => [
            format!("SET SESSION TRANSACTION ISOLATION LEVEL {level}"),
            if provider == DbDriverKind::MariaDb {
                "SELECT @@SESSION.tx_isolation AS transaction_isolation".into()
            } else {
                "SELECT @@SESSION.transaction_isolation AS transaction_isolation".into()
            },
        ],
        DbDriverKind::Sqlite => [
            "PRAGMA read_uncommitted = 0".into(),
            "PRAGMA read_uncommitted".into(),
        ],
        _ => unreachable!("unsupported providers rejected above"),
    })
}

fn isolation_change_allowed(state: &str) -> bool {
    matches!(state, "Idle" | "Session closed")
}

fn autocommit_statements(
    provider: DbDriverKind,
    enabled: bool,
    state: &str,
) -> Result<[String; 2], String> {
    if !matches!(provider, DbDriverKind::MySql | DbDriverKind::MariaDb) {
        return Err(
            "Native autocommit controls are currently available for MySQL/MariaDB only".into(),
        );
    }
    check_autocommit_change(enabled, state)?;
    Ok([
        format!("SET SESSION autocommit = {}", u8::from(enabled)),
        "SELECT @@SESSION.autocommit AS autocommit".into(),
    ])
}

fn check_autocommit_change(enabled: bool, state: &str) -> Result<(), String> {
    if enabled && !isolation_change_allowed(state) {
        Err("Commit or Rollback before enabling autocommit".into())
    } else {
        Ok(())
    }
}

fn apply_client_mode(tab: &mut TabState, enabled: bool) -> Result<(), String> {
    check_autocommit_change(enabled, &tab.transaction_state)?;
    tab.manual_transactions = !enabled;
    tab.script_status = Some(
        if enabled {
            "Client autocommit on; statements run without a client-managed transaction"
        } else {
            "Client autocommit off; ordinary SQL opens a transaction until Commit or Rollback"
        }
        .into(),
    );
    Ok(())
}

fn setting_statements(
    provider: DbDriverKind,
    setting: SessionSetting,
    state: &str,
) -> Result<[String; 2], String> {
    match setting {
        SessionSetting::Isolation(level) => {
            if !isolation_change_allowed(state) {
                return Err("Commit or Rollback before changing isolation".into());
            }
            isolation_statements(provider, level)
        }
        SessionSetting::AutoCommit(enabled) => autocommit_statements(provider, enabled, state),
    }
}

impl Workspace {
    fn apply_session_setting(
        &mut self,
        tab_id: u64,
        provider: DbDriverKind,
        driver: Arc<dyn DatabaseDriver>,
        setting: SessionSetting,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id && tab.is_script() && !tab.file_busy)
        else {
            return;
        };
        // A prompt can outlive a disconnect or connection replacement.
        let same_driver = self
            .tab_session(index)
            .driver
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &driver));
        if !same_driver || self.tab_provider(index) != provider {
            self.tabs[index].last_error = Some(
                "Connection changed while choosing session settings; reopen the selector".into(),
            );
            cx.notify();
            return;
        }
        if self.execution_busy(cx) {
            return;
        }
        if let SessionSetting::AutoCommit(enabled) = setting
            && super::transaction_mode::client_managed(provider)
        {
            if let Err(error) = apply_client_mode(&mut self.tabs[index], enabled) {
                self.tabs[index].last_error = Some(error);
            }
            cx.notify();
            return;
        }
        match setting_statements(provider, setting, &self.tabs[index].transaction_state) {
            Ok(statements) => {
                self.start_pinned_session_settings(index, driver.clone(), statements.into(), cx)
            }
            Err(error) => {
                self.tabs[index].last_error = Some(error);
                cx.notify();
            }
        }
    }

    pub(super) fn choose_session_isolation(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id && tab.is_script() && !tab.file_busy)
        else {
            return;
        };
        if !isolation_change_allowed(&self.tabs[index].transaction_state) {
            self.tabs[index].last_error =
                Some("Commit or Rollback before changing isolation".into());
            cx.notify();
            return;
        }
        let provider = self.tab_provider(index);
        let levels = isolation_levels(provider);
        let Some(driver) = self
            .tab_session(index)
            .driver
            .clone()
            .filter(|driver| driver.supports_script_session())
        else {
            return;
        };
        if levels.is_empty() {
            return;
        }
        let choices: Vec<_> = std::iter::once("Cancel")
            .chain(levels.iter().map(|level| level.label()))
            .collect();
        let preview = levels
            .iter()
            .map(|level| {
                isolation_statements(provider, *level)
                    .map(|sql| format!("{}: {}", level.label(), sql[0]))
            })
            .collect::<Result<Vec<_>, _>>();
        let preview = match preview {
            Ok(preview) => preview.join("\n"),
            Err(error) => {
                self.tabs[index].last_error = Some(error);
                cx.notify();
                return;
            }
        };
        let answer = window.prompt(PromptLevel::Info, "Set SQL tab isolation", Some(&format!("Applies to future transactions on this SQL tab. The final result reads back the server setting.\n\n{preview}")), &choices, cx);
        cx.spawn(async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            let Some(level) = choice
                .checked_sub(1)
                .and_then(|index| levels.get(index))
                .copied()
            else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.apply_session_setting(
                    tab_id,
                    provider,
                    driver,
                    SessionSetting::Isolation(level),
                    cx,
                );
            });
        })
        .detach();
    }

    pub(super) fn choose_session_autocommit(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id && tab.is_script() && !tab.file_busy)
        else {
            return;
        };
        let provider = self.tab_provider(index);
        if !matches!(
            provider,
            DbDriverKind::Postgres
                | DbDriverKind::Sqlite
                | DbDriverKind::MySql
                | DbDriverKind::MariaDb
        ) {
            return;
        }
        let Some(driver) = self
            .tab_session(index)
            .driver
            .clone()
            .filter(|driver| driver.supports_script_session())
        else {
            return;
        };
        let can_enable = isolation_change_allowed(&self.tabs[index].transaction_state);
        let choices: &[&str] = if can_enable {
            &["Cancel", "Off", "On"]
        } else {
            &["Cancel", "Off"]
        };
        let message = if super::transaction_mode::client_managed(provider) {
            "SQL tab only. Off opens a transaction before ordinary SQL when idle, including after Commit/Rollback. It stays selected while the tab is open. On is blocked during active/unknown transactions; it never commits pending work. No SQL runs just by changing this client setting."
        } else if can_enable {
            "SQL tab only. Off: SET SESSION autocommit = 0. On: SET SESSION autocommit = 1. The final result reads back @@SESSION.autocommit. Commit or Rollback is required before enabling during a transaction."
        } else {
            "On is blocked until you Commit or Rollback. Off runs SET SESSION autocommit = 0 on this SQL tab and reads back @@SESSION.autocommit; it does not end your transaction."
        };
        let answer = window.prompt(
            PromptLevel::Info,
            "Set SQL tab autocommit",
            Some(message),
            choices,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            let enabled = match choice {
                1 => false,
                2 if can_enable => true,
                _ => return,
            };
            let _ = this.update(cx, |this, cx| {
                this.apply_session_setting(
                    tab_id,
                    provider,
                    driver,
                    SessionSetting::AutoCommit(enabled),
                    cx,
                );
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{IsolationLevel, isolation_change_allowed, isolation_levels, isolation_statements};
    use db_wire::{DatabaseDriver, DbDriverKind, StreamEvent};
    use std::sync::Arc;

    #[test]
    fn client_mode_changes_preserve_pending_work_and_require_explicit_recovery() {
        let script = crate::workspace::ScriptFile {
            path: std::path::PathBuf::from("/tmp/manual-mode.sql"),
            connection_id: "test".into(),
            name: "manual-mode".into(),
            modified: std::time::SystemTime::UNIX_EPOCH,
            preview: String::new(),
        };
        let mut tab = crate::workspace::TabState::script(
            1,
            &script,
            "SELECT 1",
            ui_core::theme::Theme::dark(),
            Arc::new(sql_intel::SchemaCache::new().unwrap()),
        );
        super::apply_client_mode(&mut tab, false).unwrap();
        assert!(tab.manual_transactions);
        for state in [
            "In transaction",
            "Unknown",
            super::super::transaction_mode::FAILED_TRANSACTION_STATE,
        ] {
            tab.transaction_state = state.into();
            assert!(super::apply_client_mode(&mut tab, true).is_err());
            assert!(tab.manual_transactions);
            assert_eq!(tab.transaction_state, state);
            super::apply_client_mode(&mut tab, false).unwrap();
            assert_eq!(tab.transaction_state, state);
        }
        for state in ["Idle", "Session closed"] {
            tab.transaction_state = state.into();
            super::apply_client_mode(&mut tab, true).unwrap();
            assert!(!tab.manual_transactions);
            assert_eq!(tab.transaction_state, state);
        }
    }

    async fn apply_autocommit(
        session: &dyn db_wire::script::ScriptSession,
        enabled: bool,
        state: &str,
    ) -> Result<Vec<u8>, String> {
        let mut readback = None;
        for sql in super::autocommit_statements(DbDriverKind::MySql, enabled, state)? {
            let (tx, rx) = flume::unbounded();
            session
                .execute_streaming(&sql, tx)
                .await
                .map_err(|error| error.to_string())?;
            for event in rx.drain() {
                if let StreamEvent::SchemaReady(pager) = event {
                    readback = pager.get_cell(0, 0).or(readback);
                }
            }
        }
        readback.ok_or_else(|| "Autocommit read-back returned no value".into())
    }

    #[test]
    fn mysql_autocommit_controls_preserve_pending_work_until_explicit_rollback() {
        let Ok(port) = std::env::var("FASTDB_TEST_MYSQL_PORT") else {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "FASTDB_TEST_MYSQL_PORT required"
            );
            eprintln!("skipped: FASTDB_TEST_MYSQL_PORT not set");
            return;
        };
        let config = db_wire::ConnectionConfig {
            driver: DbDriverKind::MySql,
            host: std::env::var("FASTDB_TEST_MYSQL_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: port.parse().unwrap(),
            database: std::env::var("FASTDB_TEST_MYSQL_DATABASE")
                .unwrap_or_else(|_| "mysql".into()),
            user: std::env::var("FASTDB_TEST_MYSQL_USER").unwrap_or_else(|_| "root".into()),
            password: std::env::var("FASTDB_TEST_MYSQL_PASSWORD").ok(),
            use_unix_socket: false,
            ..Default::default()
        };
        db_wire::get_runtime().block_on(async {
            let driver = db_wire::MySqlDriver::new(config);
            let (before, _, _) = driver
                .execute_query("SELECT @@SESSION.autocommit")
                .await
                .unwrap();
            let before = before.get_cell(0, 0);
            let session = driver.open_script_session().await.unwrap();
            async fn execute(
                session: &dyn db_wire::script::ScriptSession,
                sql: &str,
            ) -> Option<Vec<u8>> {
                let (tx, rx) = flume::unbounded();
                session.execute_streaming(sql, tx).await.unwrap();
                rx.drain().find_map(|event| match event {
                    StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
                    _ => None,
                })
            }
            execute(
                session.as_ref(),
                "CREATE TEMPORARY TABLE fastdb_autocommit (id INTEGER PRIMARY KEY) ENGINE=InnoDB",
            )
            .await;
            assert_eq!(
                apply_autocommit(session.as_ref(), false, "Idle")
                    .await
                    .unwrap(),
                b"0"
            );
            assert_eq!(session.in_transaction(), Some(false));
            assert_eq!(
                apply_autocommit(session.as_ref(), true, "Idle")
                    .await
                    .unwrap(),
                b"1"
            );
            execute(session.as_ref(), "BEGIN").await;
            execute(session.as_ref(), "INSERT INTO fastdb_autocommit VALUES (1)").await;
            assert_eq!(session.in_transaction(), Some(true));
            assert_eq!(
                apply_autocommit(session.as_ref(), false, "In transaction")
                    .await
                    .unwrap(),
                b"0"
            );
            assert_eq!(session.in_transaction(), Some(true));
            assert!(
                apply_autocommit(session.as_ref(), true, "In transaction")
                    .await
                    .unwrap_err()
                    .contains("Commit or Rollback")
            );
            assert_eq!(
                execute(session.as_ref(), "SELECT @@SESSION.autocommit").await,
                Some(b"0".to_vec())
            );
            execute(session.as_ref(), "ROLLBACK").await;
            assert_eq!(
                execute(session.as_ref(), "SELECT count(*) FROM fastdb_autocommit").await,
                Some(b"0".to_vec())
            );
            // Even a read under manual mode starts a transaction; finish it first.
            execute(session.as_ref(), "ROLLBACK").await;
            assert_eq!(session.in_transaction(), Some(false));
            // Manual mode must also start transactions without an explicit BEGIN.
            execute(session.as_ref(), "INSERT INTO fastdb_autocommit VALUES (3)").await;
            assert_eq!(session.in_transaction(), Some(true));
            assert!(
                apply_autocommit(session.as_ref(), true, "In transaction")
                    .await
                    .is_err()
            );
            execute(session.as_ref(), "ROLLBACK").await;
            assert_eq!(
                execute(session.as_ref(), "SELECT count(*) FROM fastdb_autocommit").await,
                Some(b"0".to_vec())
            );
            execute(session.as_ref(), "ROLLBACK").await;
            assert_eq!(
                apply_autocommit(session.as_ref(), true, "Idle")
                    .await
                    .unwrap(),
                b"1"
            );
            execute(session.as_ref(), "INSERT INTO fastdb_autocommit VALUES (2)").await;
            execute(session.as_ref(), "ROLLBACK").await;
            assert_eq!(
                execute(session.as_ref(), "SELECT sum(id) FROM fastdb_autocommit").await,
                Some(b"2".to_vec())
            );
            let (after, _, _) = driver
                .execute_query("SELECT @@SESSION.autocommit")
                .await
                .unwrap();
            assert_eq!(after.get_cell(0, 0), before);
            session.finish().await.unwrap();
        });
    }

    #[test]
    fn autocommit_enable_requires_an_explicitly_finished_transaction() {
        for state in [
            "In transaction",
            "Transaction failed; Rollback or ROLLBACK TO required",
            "Unknown",
            "",
        ] {
            assert!(super::autocommit_statements(DbDriverKind::MySql, true, state).is_err());
            assert!(super::autocommit_statements(DbDriverKind::MySql, false, state).is_ok());
        }
        assert_eq!(
            super::autocommit_statements(DbDriverKind::MySql, true, "Idle").unwrap(),
            [
                "SET SESSION autocommit = 1",
                "SELECT @@SESSION.autocommit AS autocommit"
            ]
        );
        assert!(super::autocommit_statements(DbDriverKind::Postgres, false, "Idle").is_err());
    }

    async fn verify_isolation(driver: Arc<dyn DatabaseDriver>, provider: DbDriverKind) {
        let session = driver.open_script_session().await.unwrap();
        for level in isolation_levels(provider) {
            let statements = isolation_statements(provider, *level).unwrap();
            let (before, _, _) = driver.execute_query(&statements[1]).await.unwrap();
            let before = before.get_cell(0, 0);
            for (index, sql) in statements.iter().enumerate() {
                let (tx, rx) = flume::unbounded();
                session.execute_streaming(sql, tx).await.unwrap();
                if index == 1 {
                    let actual = rx
                        .drain()
                        .find_map(|event| match event {
                            StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
                            _ => None,
                        })
                        .unwrap();
                    let expected = match provider {
                        DbDriverKind::Sqlite => "0".into(),
                        DbDriverKind::Postgres => level.label().to_ascii_lowercase(),
                        _ => level.label().to_ascii_uppercase().replace(' ', "-"),
                    };
                    assert_eq!(actual, expected.as_bytes());
                }
            }
            let (after, _, _) = driver.execute_query(&statements[1]).await.unwrap();
            assert_eq!(
                after.get_cell(0, 0),
                before,
                "tab isolation must not change the browsing session"
            );
        }
        session.finish().await.unwrap();
    }

    #[test]
    fn sqlite_isolation_readback_uses_the_private_tab_connection() {
        db_wire::get_runtime().block_on(async {
            let driver = Arc::new(db_wire::SqliteDriver::new_memory());
            driver
                .execute_query("PRAGMA read_uncommitted = 1")
                .await
                .unwrap();
            verify_isolation(driver, DbDriverKind::Sqlite).await;
        });
    }

    #[test]
    fn live_server_isolation_readback_stays_on_the_tab_session() {
        for (prefix, provider) in [
            ("MYSQL", DbDriverKind::MySql),
            ("PG", DbDriverKind::Postgres),
        ] {
            let env = |key: &str| std::env::var(format!("FASTDB_TEST_{prefix}_{key}"));
            let Ok(port) = env("PORT") else {
                assert!(
                    std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                    "FASTDB_TEST_{prefix}_PORT required"
                );
                eprintln!("skipped: FASTDB_TEST_{prefix}_PORT not set");
                continue;
            };
            let config = db_wire::ConnectionConfig {
                driver: provider,
                host: env("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
                port: port.parse().unwrap(),
                database: env("DATABASE").unwrap_or_else(|_| {
                    if provider == DbDriverKind::Postgres {
                        "postgres"
                    } else {
                        "mysql"
                    }
                    .into()
                }),
                user: env("USER").unwrap_or_else(|_| {
                    if provider == DbDriverKind::Postgres {
                        "postgres"
                    } else {
                        "root"
                    }
                    .into()
                }),
                password: env("PASSWORD").ok(),
                use_unix_socket: false,
                ..Default::default()
            };
            db_wire::get_runtime().block_on(verify_isolation(
                db_wire::create_driver(config).unwrap(),
                provider,
            ));
        }
    }

    #[test]
    fn isolation_controls_offer_only_supported_levels_and_require_idle_state() {
        assert_eq!(
            isolation_levels(DbDriverKind::Sqlite),
            &[IsolationLevel::Serializable]
        );
        assert_eq!(isolation_levels(DbDriverKind::Postgres).len(), 3);
        assert_eq!(isolation_levels(DbDriverKind::MySql).len(), 4);
        assert!(isolation_levels(DbDriverKind::Oracle).is_empty());
        assert!(
            isolation_statements(DbDriverKind::Postgres, IsolationLevel::ReadUncommitted).is_err()
        );
        assert_eq!(
            isolation_statements(DbDriverKind::MySql, IsolationLevel::RepeatableRead).unwrap(),
            [
                "SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ",
                "SELECT @@SESSION.transaction_isolation AS transaction_isolation"
            ]
        );
        assert!(isolation_change_allowed("Idle"));
        assert!(isolation_change_allowed("Session closed"));
        for state in [
            "In transaction",
            "Transaction failed; Rollback or ROLLBACK TO required",
            "",
            "Unknown",
        ] {
            assert!(!isolation_change_allowed(state));
        }
    }
}
