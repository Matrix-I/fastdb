use super::{EstablishedConnection, PostgresDriver, SocketGuard, pg_error_message};
use crate::DatabaseDriver;
use crate::administration::*;
use anyhow::{Context, Result};
use futures::TryStreamExt;
use std::{sync::Arc, time::Duration};
use tokio_postgres::{Client, SimpleQueryMessage};

const DETAIL_CHAR_LIMIT: usize = 8192;

fn bounded_details(details: String) -> String {
    if details.chars().nth(DETAIL_CHAR_LIMIT).is_some() {
        let mut preview = details.chars().take(DETAIL_CHAR_LIMIT).collect::<String>();
        preview.push_str("\nDetails truncated at 8192 characters.");
        preview
    } else {
        details
    }
}

fn catalog_sql(family: AdminFamily) -> &'static str {
    match family {
        AdminFamily::Routines => include_str!("administration/routines.sql"),
        AdminFamily::Sequences => include_str!("administration/sequences.sql"),
        AdminFamily::Triggers => include_str!("administration/triggers.sql"),
        AdminFamily::Rules => include_str!("administration/rules.sql"),
        AdminFamily::Policies => include_str!("administration/policies.sql"),
        AdminFamily::Partitions => include_str!("administration/partitions.sql"),
        AdminFamily::Roles => include_str!("administration/roles.sql"),
        AdminFamily::Privileges => include_str!("administration/privileges.sql"),
    }
}

impl PostgresDriver {
    pub(super) async fn read_administration_catalog(
        &self,
        family: AdminFamily,
        after: u32,
    ) -> Result<AdminPage> {
        tokio::time::timeout(Duration::from_secs(15), async {
            let (client, task) = Self::connect_with_task(self.config.clone()).await?;
            let _socket = SocketGuard(task);
            client.batch_execute("SET search_path=pg_catalog; SET statement_timeout='10s'; BEGIN READ ONLY").await?;
            let sql = format!("SELECT id, schema, name, parent, left(details,$4), CASE WHEN octet_length(source)<=$3 THEN source END, invocation FROM ({}) AS objects WHERE id>$1 ORDER BY id LIMIT $2", catalog_sql(family));
            let rows = client.query(&sql, &[&after, &((ADMIN_PAGE_SIZE + 1) as i64), &(ADMIN_SQL_LIMIT as i32), &((DETAIL_CHAR_LIMIT + 1) as i32)]).await
                .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
            let more = rows.len() > ADMIN_PAGE_SIZE;
            let objects: Vec<_> = rows.into_iter().take(ADMIN_PAGE_SIZE).map(|row| AdminObject {
                id: row.get(0), schema: row.get(1), name: row.get(2), parent: row.get(3),
                details: bounded_details(row.get(4)), source: row.get(5), invocation: row.get(6),
            }).collect();
            let next = more.then(|| objects.last().unwrap().id);
            Ok(AdminPage { objects, next })
        }).await.context("Administration catalog timed out")?
    }

    pub(super) async fn run_administration(
        &self,
        sql: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<AdminOutcome> {
        anyhow::ensure!(
            !self.is_read_only(),
            "Administration execution is disabled for read-only profiles"
        );
        validate_admin_sql(sql)?;
        let EstablishedConnection {
            client,
            task,
            cancellation,
        } = tokio::select! {
            biased;
            _ = cancel.recv_async() => anyhow::bail!("Administration canceled before connecting"),
            connection = Self::connect_with_notices(self.config.clone(), Arc::default()) => connection?,
        };
        let _socket = SocketGuard(task);
        let result = tokio::select! {
            biased;
            _ = cancel.recv_async() => Err(anyhow::anyhow!("Administration canceled")),
            result = tokio::time::timeout(Duration::from_secs(20), execute_statement(&client, sql)) => {
                result.unwrap_or_else(|_| Err(anyhow::anyhow!("Administration timed out")))
            }
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                let cancellation_status =
                    match tokio::time::timeout(Duration::from_secs(2), cancellation.cancel()).await
                    {
                        Ok(Ok(())) => "Cancellation request sent",
                        Ok(Err(_)) => "Server cancellation failed",
                        Err(_) => "Server cancellation timed out",
                    };
                let rollback =
                    tokio::time::timeout(Duration::from_secs(3), client.batch_execute("ROLLBACK"))
                        .await;
                return match rollback {
                    Ok(Ok(())) => Err(error.context(format!("{cancellation_status}. Transaction rolled back; external routine effects and sequence nextval/setval may persist"))),
                    _ => Err(error.context("Connection closed; verify server state before retrying")),
                };
            }
        };
        if cancel.try_recv().is_ok() || cancel.is_disconnected() {
            tokio::time::timeout(Duration::from_secs(3), client.batch_execute("ROLLBACK"))
                .await
                .context("Rollback timed out; verify server state")??;
            anyhow::bail!(
                "Administration canceled and rolled back; external routine effects and sequence nextval/setval may persist"
            );
        }
        // Once COMMIT is sent, cancellation cannot establish whether it committed.
        tokio::time::timeout(Duration::from_secs(5), client.batch_execute("COMMIT"))
            .await
            .context("Commit outcome unknown; refresh before retrying")?
            .map_err(|_| anyhow::anyhow!("Commit was not acknowledged; refresh before retrying"))?;
        Ok(outcome)
    }
}

async fn execute_statement(client: &Client, sql: &str) -> Result<AdminOutcome> {
    client.batch_execute("SET search_path=pg_catalog,public; SET statement_timeout='15s'; SET lock_timeout='5s'; BEGIN").await?;
    // Parse rejects multiple statements, including COMMIT injected after a DDL
    // statement. Unlike splitting on semicolons, it accepts routine dollar bodies.
    let statement = client
        .prepare(sql)
        .await
        .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
    anyhow::ensure!(
        statement.params().is_empty(),
        "Replace argument placeholders with SQL values before reviewing"
    );
    let stream = client
        .simple_query_raw(sql)
        .await
        .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
    futures::pin_mut!(stream);
    let mut outcome = AdminOutcome::default();
    while let Some(message) = stream
        .try_next()
        .await
        .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?
    {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                outcome.truncated |= columns.len() > ADMIN_RESULT_COLUMNS;
                outcome.columns = columns
                    .iter()
                    .take(ADMIN_RESULT_COLUMNS)
                    .map(|c| c.name().to_owned())
                    .collect();
            }
            SimpleQueryMessage::Row(row) => {
                if outcome.rows.len() < ADMIN_RESULT_ROWS {
                    let values = (0..row.len().min(ADMIN_RESULT_COLUMNS))
                        .map(|i| {
                            row.get(i).map(|value| {
                                outcome.truncated |= value.chars().count() > ADMIN_CELL_CHARS;
                                value.chars().take(ADMIN_CELL_CHARS).collect()
                            })
                        })
                        .collect();
                    outcome.rows.push(values);
                } else {
                    outcome.truncated = true;
                }
            }
            SimpleQueryMessage::CommandComplete(count) => outcome.affected = count,
            _ => {}
        }
    }
    Ok(outcome)
}
