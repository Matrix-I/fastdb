use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;
use mem_engine::{ColumnMeta, QueryResultPager};

use crate::traits::StreamEvent;

/// Minimum spacing between PageAppended notifications (one 60 FPS frame)
const NOTIFY_INTERVAL: Duration = Duration::from_millis(16);

/// Sends `msg` as a StreamEvent::Error and returns it as an error for the caller to propagate
pub(crate) fn report_error(event_tx: &flume::Sender<StreamEvent>, msg: impl Into<String>) -> anyhow::Error {
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
    pub fn start(columns: Vec<ColumnMeta>, event_tx: flume::Sender<StreamEvent>, started_at: Instant) -> Result<Self> {
        let num_cols = columns.len();
        let pager = match QueryResultPager::new(columns) {
            Ok(p) => Arc::new(p),
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
            let _ = self.event_tx.send(StreamEvent::PageAppended { total_rows });
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

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::DataType;

    #[test]
    fn test_row_sink_emits_events_in_order() {
        let (event_tx, event_rx) = flume::unbounded();
        let columns = vec![ColumnMeta::new("id", DataType::Int4), ColumnMeta::new("note", DataType::Text)];
        let mut sink = RowSink::start(columns, event_tx, Instant::now()).unwrap();

        // ~20KB rows fill a 64KB page every 3-4 rows, forcing two rollovers that must be announced
        let note = "x".repeat(20_000);
        for i in 0..10 {
            let cells = sink.cells_mut();
            cells[0] = Some(i.to_string().into_bytes());
            cells[1] = if i == 3 { None } else { Some(note.clone().into_bytes()) };
            sink.commit_row().unwrap();
        }
        assert_eq!(sink.finish(), 10);

        let events: Vec<StreamEvent> = event_rx.drain().collect();
        let StreamEvent::SchemaReady(pager) = &events[0] else {
            panic!("first event must be SchemaReady, got {:?}", events[0]);
        };
        let appended = events.iter().filter(|e| matches!(e, StreamEvent::PageAppended { .. })).count();
        assert!(appended >= 2, "expected one notification per page rollover, got {appended}");
        assert!(matches!(events.last(), Some(StreamEvent::Completed { total_rows: 10, .. })));
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
