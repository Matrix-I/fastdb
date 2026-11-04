//! Executed table states, separate from text-input undo and SQL query history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FilterSnapshot {
    pub sql: String,
    pub order: Vec<ui_grid::ColumnOrder>,
    pub fetch_size: usize,
    pub criteria: Option<db_wire::filter::FilterSpec>,
}
#[derive(Default)]
pub(super) struct FilterTimeline {
    entries: Vec<FilterSnapshot>,
    cursor: Option<usize>,
    replaying: bool,
}
impl FilterTimeline {
    pub fn record(&mut self, snapshot: FilterSnapshot) {
        if self.replaying {
            self.replaying = false;
            return;
        }
        if self
            .cursor
            .is_some_and(|i| self.entries.get(i) == Some(&snapshot))
        {
            return;
        }
        if let Some(i) = self.cursor {
            self.entries.truncate(i + 1);
        }
        self.entries.push(snapshot);
        while self.entries.len() > 1
            && (self.entries.len() > 100
                || self
                    .entries
                    .iter()
                    .map(|s| {
                        s.sql.len()
                            + s.order.iter().map(|key| key.column.len()).sum::<usize>()
                            + s.criteria
                                .as_ref()
                                .map(|c| c.criteria.iter().map(|v| v.value.len()).sum::<usize>())
                                .unwrap_or(0)
                    })
                    .sum::<usize>()
                    > 1024 * 1024)
        {
            self.entries.remove(0);
        }
        self.cursor = Some(self.entries.len() - 1);
    }
    pub fn can_back(&self) -> bool {
        self.cursor.is_some_and(|i| i > 0)
    }
    pub fn can_forward(&self) -> bool {
        self.cursor.is_some_and(|i| i + 1 < self.entries.len())
    }
    pub fn navigate(&mut self, forward: bool) -> Option<FilterSnapshot> {
        let i = self.cursor?;
        let next = if forward && self.can_forward() {
            i + 1
        } else if !forward && self.can_back() {
            i - 1
        } else {
            return None;
        };
        self.cursor = Some(next);
        self.replaying = true;
        Some(self.entries[next].clone())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(value: &str) -> FilterSnapshot {
        FilterSnapshot {
            sql: value.into(),
            order: Vec::new(),
            fetch_size: 200,
            criteria: None,
        }
    }
    #[test]
    fn replay_retains_forward_branch_and_new_execution_replaces_it() {
        let mut h = FilterTimeline::default();
        for sql in ["", "a", "b"] {
            h.record(snapshot(sql));
        }
        assert_eq!(h.navigate(false).unwrap().sql, "a");
        h.record(snapshot("a"));
        assert!(h.can_forward());
        assert_eq!(h.navigate(true).unwrap().sql, "b");
        h.record(snapshot("b"));
        h.navigate(false);
        h.record(snapshot("a"));
        h.record(snapshot("new"));
        assert!(!h.can_forward());
        assert!(h.can_back());
        for i in 0..150 {
            h.record(snapshot(&i.to_string()));
        }
        assert_eq!(h.entries.len(), 100);
    }
}
