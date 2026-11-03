//! Bounded statement feedback independent of retained result grids.

use db_wire::StreamEvent;
use std::{collections::VecDeque, time::Instant};

const MAX_ENTRIES: usize = 100;
const MAX_MESSAGES: usize = 21;
const MAX_TEXT: usize = 1024;
const MAX_SQL_PREVIEW: usize = 240;

#[derive(Default)]
pub(super) struct ExecutionLog {
    entries: VecDeque<Entry>,
    active: bool,
}
struct Entry {
    sql: String,
    started: Instant,
    outcome: String,
    command: Option<Option<u64>>,
    messages: Vec<String>,
    display: gpui::SharedString,
}
impl ExecutionLog {
    pub fn start(&mut self, sql: &str) {
        if self.entries.len() == MAX_ENTRIES {
            self.entries.pop_front();
        }
        let preview: String = sql.chars().take(MAX_SQL_PREVIEW).collect();
        self.entries.push_back(Entry {
            display: format!("{preview}\nRunning…").into(),
            sql: preview,
            started: Instant::now(),
            outcome: "Running…".into(),
            command: None,
            messages: Vec::new(),
        });
        self.active = true;
    }
    pub fn apply(&mut self, event: &StreamEvent) {
        if !self.active {
            return;
        }
        let Some(entry) = self.entries.back_mut() else {
            return;
        };
        match event {
            StreamEvent::CommandComplete { affected_rows } => entry.command = Some(*affected_rows),
            StreamEvent::Notice(message) if entry.messages.len() < MAX_MESSAGES => {
                entry
                    .messages
                    .push(message.chars().take(MAX_TEXT).collect());
            }
            StreamEvent::Completed {
                total_rows,
                elapsed_ms,
            } => {
                let count = match entry.command {
                    Some(Some(count)) => format!("{count} row(s) affected"),
                    Some(None) => "affected count unavailable".into(),
                    None => format!("{total_rows} row(s) returned"),
                };
                entry.outcome = format!("{count} · {elapsed_ms} ms");
                self.active = false;
            }
            StreamEvent::Error(message) => {
                let message: String = message.chars().take(MAX_TEXT).collect();
                entry.outcome = format!(
                    "Failed · {} ms · {message}",
                    entry.started.elapsed().as_millis()
                );
                self.active = false;
            }
            _ => return,
        }
        entry.refresh();
    }
    pub fn is_active(&self) -> bool {
        self.active
    }
    pub fn messages(&self) -> impl DoubleEndedIterator<Item = gpui::SharedString> + '_ {
        self.entries.iter().map(|entry| entry.display.clone())
    }
}
impl Entry {
    fn refresh(&mut self) {
        let mut message = format!("{}\n{}", self.sql, self.outcome);
        for notice in &self.messages {
            message.push('\n');
            message.push_str(notice);
        }
        self.display = message.into();
    }
}

/// Per-tab presets avoid parsing ambiguous units and never depend on wall-clock time.
pub(super) const TIMEOUT_SECONDS: [u64; 5] = [0, 5, 30, 60, 300];

#[cfg(test)]
mod tests {
    use super::*;
    use db_wire::StreamEvent;

    #[test]
    fn later_failure_keeps_earlier_command_count_and_notice() {
        let mut log = ExecutionLog::default();
        log.start("UPDATE items SET active = true");
        log.apply(&StreamEvent::CommandComplete {
            affected_rows: Some(3),
        });
        log.apply(&StreamEvent::Notice("NOTICE: updated".into()));
        log.apply(&StreamEvent::Completed {
            total_rows: 0,
            elapsed_ms: 12,
        });
        log.start("broken SQL");
        log.apply(&StreamEvent::Error("ERROR: syntax · character 8".into()));
        let entries: Vec<_> = log.messages().collect();
        assert!(entries[0].contains("3 row(s) affected"));
        assert!(entries[0].contains("12 ms"));
        assert!(entries[0].contains("NOTICE: updated"));
        assert!(entries[1].contains("character 8"));
    }

    #[test]
    fn rows_are_distinct_from_unknown_command_counts_and_storage_is_bounded() {
        let mut log = ExecutionLog::default();
        for _ in 0..MAX_ENTRIES + 1 {
            log.start(&"é".repeat(MAX_TEXT));
            for _ in 0..MAX_MESSAGES + 1 {
                log.apply(&StreamEvent::Notice("x".repeat(MAX_TEXT + 1)));
            }
            log.apply(&StreamEvent::Completed {
                total_rows: 2,
                elapsed_ms: 4,
            });
        }
        assert_eq!(log.entries.len(), MAX_ENTRIES);
        assert_eq!(log.entries[0].messages.len(), MAX_MESSAGES);
        assert!(log.entries[0].sql.chars().count() <= MAX_SQL_PREVIEW);
        assert!(log.messages().next().unwrap().contains("2 row(s) returned"));
        log.start("DDL");
        log.apply(&StreamEvent::CommandComplete {
            affected_rows: None,
        });
        log.apply(&StreamEvent::Completed {
            total_rows: 0,
            elapsed_ms: 1,
        });
        assert!(
            log.messages()
                .last()
                .unwrap()
                .contains("affected count unavailable")
        );
    }
}
