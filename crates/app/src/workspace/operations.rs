use db_wire::DatabaseDriver;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OperationKind {
    Query,
    Save,
}

pub(super) struct Operation {
    pub id: u64,
    pub tab_id: u64,
    pub connection_id: String,
    pub driver: Arc<dyn DatabaseDriver>,
    pub kind: OperationKind,
    pub cancel_requested: bool,
}

#[derive(Default)]
pub(super) struct Operations {
    next_id: u64,
    active: Option<Operation>,
}
impl Operations {
    pub fn is_running(&self) -> bool {
        self.active.is_some()
    }
    pub fn active(&self) -> Option<&Operation> {
        self.active.as_ref()
    }
    pub fn begin(
        &mut self,
        tab_id: u64,
        connection_id: String,
        driver: Arc<dyn DatabaseDriver>,
        kind: OperationKind,
    ) -> Option<u64> {
        if self.is_running() {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.active = Some(Operation {
            id,
            tab_id,
            connection_id,
            driver,
            kind,
            cancel_requested: false,
        });
        Some(id)
    }
    pub fn matches(&self, id: u64, tab_id: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|op| op.id == id && op.tab_id == tab_id)
    }
    pub fn finish(&mut self, id: u64) -> bool {
        if self.active.as_ref().is_some_and(|op| op.id == id) {
            self.active = None;
            true
        } else {
            false
        }
    }
    pub fn request_cancel(&mut self) -> Option<(u64, Arc<dyn DatabaseDriver>)> {
        let op = self.active.as_mut()?;
        if op.cancel_requested || op.kind == OperationKind::Save {
            return None;
        }
        op.cancel_requested = true;
        Some((op.id, op.driver.clone()))
    }
    pub fn cancel_failed(&mut self, id: u64) {
        if let Some(op) = self.active.as_mut().filter(|op| op.id == id) {
            op.cancel_requested = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationKind, Operations};
    use db_wire::{ConnectionConfig, create_driver};
    use std::sync::Arc;
    #[test]
    fn stale_completion_and_cancellation_keep_the_original_owner() {
        let driver = create_driver(ConnectionConfig {
            driver: db_wire::DbDriverKind::Sqlite,
            database: ":memory:".into(),
            ..ConnectionConfig::default()
        })
        .unwrap();
        let mut runs = Operations::default();
        let first = runs
            .begin(
                7,
                "connection-a".into(),
                driver.clone(),
                OperationKind::Query,
            )
            .unwrap();
        assert!(
            runs.begin(
                8,
                "connection-b".into(),
                driver.clone(),
                OperationKind::Query
            )
            .is_none()
        );
        assert!(!runs.matches(first, 8));
        assert_eq!(runs.active().unwrap().connection_id, "connection-a");
        let (id, cancellation_driver) = runs.request_cancel().unwrap();
        assert_eq!(id, first);
        assert!(Arc::ptr_eq(&driver, &cancellation_driver));
        assert!(runs.request_cancel().is_none());
        runs.cancel_failed(first);
        assert!(runs.request_cancel().is_some());
        assert!(runs.finish(first));
        let second = runs
            .begin(8, "connection-b".into(), driver, OperationKind::Save)
            .unwrap();
        assert!(!runs.finish(first));
        assert!(runs.matches(second, 8));
        assert!(runs.request_cancel().is_none());
        assert!(runs.finish(second));
    }
}
