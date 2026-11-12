//! SQL Server estimated plans use three batches on one leased ODBC connection.
use super::*;
use crate::QueryResultPager;

const SHOWPLAN_ON: &str = "SET SHOWPLAN_ALL ON";
const SHOWPLAN_OFF: &str = "SET SHOWPLAN_ALL OFF";
const SETTING_TIMEOUT_SECONDS: usize = 5;
const MAX_ROWS: u64 = 4096;
const MAX_COLUMNS: u16 = 32;
const MAX_BYTES: usize = 16 * 1024 * 1024;

fn scoped<T>(
    mut setting: impl FnMut(&'static str) -> Result<()>,
    capture: impl FnOnce() -> Result<T>,
    discard: impl FnOnce(),
) -> Result<T> {
    if let Err(error) = setting(SHOWPLAN_ON) {
        discard();
        return Err(error.context("Cannot enable SQL Server SHOWPLAN_ALL"));
    }
    let result = capture();
    if let Err(cleanup) = setting(SHOWPLAN_OFF) {
        discard();
        return match result {
            Ok(_) => {
                Err(cleanup.context("Cannot disable SQL Server SHOWPLAN_ALL. Session discarded"))
            }
            Err(error) => Err(error.context(format!(
                "Cannot disable SQL Server SHOWPLAN_ALL. Session discarded: {cleanup:#}"
            ))),
        };
    }
    result
}

pub(super) fn capture(
    conn: &Connection<'_>,
    driver: &OdbcDriver,
    sql: &str,
) -> Result<QueryResultPager> {
    scoped(
        |setting| {
            crate::sql_observer::observe!("ODBC", setting, |query| {
                let mut statement = conn.preallocate()?;
                statement.set_query_timeout_sec(SETTING_TIMEOUT_SECONDS)?;
                *driver.active_statement.lock() = Some(statement.as_stmt_ref().as_sys().0 as usize);
                let _active = ActiveStatement(driver.active_statement.clone());
                if setting == SHOWPLAN_ON {
                    ensure_running(driver)?;
                }
                drop(statement.execute(query, ())?);
                Ok::<_, anyhow::Error>(())
            })
        },
        || capture_rows(conn, driver, sql),
        || driver.closed.store(true, Ordering::Release),
    )
}

fn ensure_running(driver: &OdbcDriver) -> Result<()> {
    anyhow::ensure!(
        !driver.cancelled.load(Ordering::Acquire) && !driver.closed.load(Ordering::Acquire),
        "Query cancelled"
    );
    Ok(())
}

fn capture_rows(conn: &Connection<'_>, driver: &OdbcDriver, sql: &str) -> Result<QueryResultPager> {
    crate::sql_observer::observe!(
        "ODBC",
        sql,
        |query| {
            let mut statement = conn.preallocate()?;
            statement.set_query_timeout_sec(script::QUERY_TIMEOUT_SECONDS)?;
            *driver.active_statement.lock() = Some(statement.as_stmt_ref().as_sys().0 as usize);
            let _active = ActiveStatement(driver.active_statement.clone());
            ensure_running(driver)?;
            let mut cursor = statement
                .execute(query, ())?
                .context("SQL Server returned no plan")?;
            let count = u16::try_from(cursor.num_result_cols()?)?;
            anyhow::ensure!(
                (1..=MAX_COLUMNS).contains(&count),
                "Invalid SQL Server plan column count"
            );
            let columns = (1..=count)
                .map(|column| {
                    let mut description = odbc_api::ColumnDescription::default();
                    cursor.describe_col(column, &mut description)?;
                    Ok(ColumnMeta::new(
                        description.name_to_string()?,
                        DataType::Text,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let pager = QueryResultPager::new(columns)?;
            let mut size = 0;
            let mut buffer = Vec::new();
            while let Some(mut row) = cursor.next_row()? {
                ensure_running(driver)?;
                anyhow::ensure!(
                    pager.total_rows() < MAX_ROWS,
                    "SQL Server plan exceeds {MAX_ROWS} rows"
                );
                let mut cells = Vec::with_capacity(count as usize);
                for column in 1..=count {
                    buffer.clear();
                    let mut chunk = odbc_api::parameter::VarWCharArray::<4096>::NULL;
                    let cell = loop {
                        row.get_data(column, &mut chunk)?;
                        let Some(units) = chunk.as_slice() else {
                            anyhow::ensure!(buffer.is_empty(), "Invalid SQL Server plan field");
                            break None;
                        };
                        anyhow::ensure!(
                            buffer.len() + units.len() <= MAX_BYTES - size,
                            "SQL Server plan exceeds 16 MiB"
                        );
                        buffer.extend_from_slice(units);
                        if chunk.is_complete() {
                            let value = String::from_utf16(&buffer)?;
                            size += value.len();
                            anyhow::ensure!(size <= MAX_BYTES, "SQL Server plan exceeds 16 MiB");
                            break Some(value);
                        }
                        anyhow::ensure!(
                            !units.is_empty(),
                            "SQL Server plan field returned no data"
                        );
                        ensure_running(driver)?;
                    };
                    cells.push(cell);
                }
                pager.append_row(cells)?;
            }
            anyhow::ensure!(
                cursor.more_results()?.is_none(),
                "SQL Server Explain requires one result set"
            );
            Ok(pager)
        },
        |pager| Some(pager.total_rows())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn showplan_cleanup_runs_after_statement_failure_and_discards_on_reset_failure() {
        let calls = RefCell::new(Vec::new());
        let result = scoped(
            |sql| {
                calls.borrow_mut().push(sql);
                if sql == SHOWPLAN_OFF {
                    bail!("reset failed");
                }
                Ok(())
            },
            || -> Result<()> {
                calls.borrow_mut().push("capture");
                bail!("statement failed")
            },
            || calls.borrow_mut().push("discard"),
        );
        assert_eq!(
            *calls.borrow(),
            [SHOWPLAN_ON, "capture", SHOWPLAN_OFF, "discard"]
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("statement failed"));
    }

    #[test]
    fn showplan_setup_failure_never_runs_original_statement() {
        let result = scoped(
            |_| bail!("permission denied"),
            || -> Result<()> { panic!("statement ran with SHOWPLAN unavailable") },
            || {},
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("permission denied"));
    }
}
