use anyhow::Result;
use mem_engine::{ColumnMeta, QueryResultPager};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::traits::StreamEvent;

/// Minimum spacing between PageAppended notifications (one 60 FPS frame)
const NOTIFY_INTERVAL: Duration = Duration::from_millis(16);
/// Reserve relay capacity for schema, bounded notices and terminal events.
pub(crate) const MAX_PENDING_PROGRESS: usize = 32;

/// Sends `msg` as a StreamEvent::Error and returns it as an error for the caller to propagate
pub(crate) fn report_error(
    event_tx: &flume::Sender<StreamEvent>,
    msg: impl Into<String>,
) -> anyhow::Error {
    let msg = msg.into();
    let _ = event_tx.send(StreamEvent::Error(msg.clone()));
    anyhow::anyhow!(msg)
}

/// Shared tail of every driver's streaming loop: owns the result pager, buffers one row of
/// cells, and emits SchemaReady, throttled PageAppended and Completed events.
pub(crate) struct RowSink {
    pager: Arc<QueryResultPager>,
    event_tx: flume::Sender<StreamEvent>,
    started_at: Instant,
    last_notified_at: Instant,
    total_rows: u64,
    cells: Vec<Option<Vec<u8>>>,
}

impl RowSink {
    /// Creates the pager for `columns` and announces it with StreamEvent::SchemaReady
    pub fn start(
        columns: Vec<ColumnMeta>,
        event_tx: flume::Sender<StreamEvent>,
        started_at: Instant,
    ) -> Result<Self> {
        Self::start_with_origin(columns, event_tx, started_at, None)
    }

    pub fn start_with_origin(
        columns: Vec<ColumnMeta>,
        event_tx: flume::Sender<StreamEvent>,
        started_at: Instant,
        origin: Option<mem_engine::QueryEditOrigin>,
    ) -> Result<Self> {
        let num_cols = columns.len();
        let pager = match QueryResultPager::new(columns) {
            Ok(mut pager) => {
                pager.query_edit_origin = origin;
                Arc::new(pager)
            }
            Err(e) => return Err(report_error(&event_tx, format!("{e:#}"))),
        };
        let _ = event_tx.send(StreamEvent::SchemaReady(Arc::clone(&pager)));
        Ok(Self {
            pager,
            event_tx,
            started_at,
            last_notified_at: Instant::now(),
            total_rows: 0,
            cells: vec![None; num_cols],
        })
    }

    /// Cell buffers for the next row, one per column; None is SQL NULL
    pub fn cells_mut(&mut self) -> &mut [Option<Vec<u8>>] {
        &mut self.cells
    }

    /// Appends the buffered cells as a row, notifying on page rollover or every frame
    pub fn commit_row(&mut self) -> Result<()> {
        let cell_slices: Vec<Option<&[u8]>> = self.cells.iter().map(|c| c.as_deref()).collect();
        let (total_rows, rollover) = match self.pager.append_row(cell_slices) {
            Ok(r) => r,
            Err(e) => return Err(report_error(&self.event_tx, format!("{e:#}"))),
        };
        self.total_rows = total_rows;

        if rollover || self.last_notified_at.elapsed() >= NOTIFY_INTERVAL {
            // Progress is replaceable: the pager owns every row and Completed
            // refreshes the final count even if the UI temporarily falls behind.
            if self.event_tx.len() < MAX_PENDING_PROGRESS {
                let _ = self.event_tx.send(StreamEvent::PageAppended { total_rows });
            }
            self.last_notified_at = Instant::now();
        }
        Ok(())
    }

    /// Reports an error that ended the stream early
    pub fn fail(&self, msg: impl Into<String>) -> anyhow::Error {
        report_error(&self.event_tx, msg)
    }

    /// Emits StreamEvent::Completed and returns the total row count
    pub fn finish(self) -> u64 {
        let elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        let _ = self.event_tx.send(StreamEvent::Completed {
            total_rows: self.total_rows,
            elapsed_ms,
        });
        self.total_rows
    }
}

/// Bound notices before they reach any unbounded event relay. Never block the
/// PostgreSQL protocol pump, and make truncation visible exactly once.
pub(crate) struct NoticeForwarder {
    tx: flume::Sender<StreamEvent>,
    sent: usize,
}
impl NoticeForwarder {
    pub fn new(tx: flume::Sender<StreamEvent>) -> Self {
        Self { tx, sent: 0 }
    }
    pub fn send(&mut self, message: String) {
        const MAX_NOTICES: usize = 20;
        const MAX_NOTICE_CHARS: usize = 1024;
        let message = if self.sent < MAX_NOTICES {
            message.chars().take(MAX_NOTICE_CHARS).collect()
        } else if self.sent == MAX_NOTICES {
            "Further server notices omitted (limit: 20 per statement)".into()
        } else {
            return;
        };
        self.sent += 1;
        let _ = self.tx.send(StreamEvent::Notice(message));
    }
}

#[cfg(test)]
mod notice_tests {
    use super::*;
    #[test]
    fn notice_flood_is_bounded_before_relay_with_truncation_feedback() {
        let (tx, rx) = flume::unbounded();
        let mut notices = NoticeForwarder::new(tx);
        for _ in 0..10_000 {
            notices.send("é".repeat(2000));
        }
        assert_eq!(rx.len(), 21);
        let first = rx.recv().unwrap();
        assert!(matches!(first, StreamEvent::Notice(message) if message.chars().count() == 1024));
        assert!(
            matches!(rx.drain().last(), Some(StreamEvent::Notice(message)) if message.contains("omitted"))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::DataType;

    #[test]
    fn slow_consumer_coalesces_progress_without_losing_result_rows() {
        let (tx, rx) = flume::bounded(64);
        let mut sink = RowSink::start(
            vec![ColumnMeta::new("note", DataType::Text)],
            tx.clone(),
            Instant::now(),
        )
        .unwrap();
        for _ in 0..MAX_PENDING_PROGRESS {
            tx.send(StreamEvent::PageAppended { total_rows: 0 })
                .unwrap();
        }
        for _ in 0..30 {
            sink.cells_mut()[0] = Some(vec![b'x'; 20_000]);
            sink.commit_row().unwrap();
        }
        assert_eq!(sink.finish(), 30);
        assert_eq!(rx.len(), MAX_PENDING_PROGRESS + 2);
        let events: Vec<_> = rx.drain().collect();
        let StreamEvent::SchemaReady(pager) = &events[0] else {
            panic!("missing schema")
        };
        assert_eq!(pager.total_rows(), 30);
        assert_eq!(pager.get_cell(29, 0).unwrap().len(), 20_000);
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Completed { total_rows: 30, .. })
        ));
    }

    #[test]
    fn test_row_sink_emits_events_in_order() {
        let (event_tx, event_rx) = flume::unbounded();
        let columns = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("note", DataType::Text),
        ];
        let mut sink = RowSink::start(columns, event_tx, Instant::now()).unwrap();

        // ~20KB rows fill a 64KB page every 3-4 rows, forcing two rollovers that must be announced
        let note = "x".repeat(20_000);
        for i in 0..10 {
            let cells = sink.cells_mut();
            cells[0] = Some(i.to_string().into_bytes());
            cells[1] = if i == 3 {
                None
            } else {
                Some(note.clone().into_bytes())
            };
            sink.commit_row().unwrap();
        }
        assert_eq!(sink.finish(), 10);

        let events: Vec<StreamEvent> = event_rx.drain().collect();
        let StreamEvent::SchemaReady(pager) = &events[0] else {
            panic!("first event must be SchemaReady, got {:?}", events[0]);
        };
        let appended = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::PageAppended { .. }))
            .count();
        assert!(
            appended >= 2,
            "expected one notification per page rollover, got {appended}"
        );
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Completed { total_rows: 10, .. })
        ));
        assert_eq!(pager.get_cell(9, 0), Some(b"9".to_vec()));
        assert_eq!(pager.get_cell(3, 1), None);
    }

    #[test]
    fn test_report_error_sends_event_and_returns_error() {
        let (event_tx, event_rx) = flume::unbounded();
        let err = report_error(&event_tx, "boom");
        assert_eq!(err.to_string(), "boom");
        assert!(matches!(event_rx.try_recv(), Ok(StreamEvent::Error(msg)) if msg == "boom"));
    }
}
