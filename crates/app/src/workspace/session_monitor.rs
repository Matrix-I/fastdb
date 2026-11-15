use super::script_tools::tool;
use super::*;
use db_wire::postgres::session_monitor::{
    SessionActivity, SessionIdentity, SessionMonitor, SessionSignal, SessionSnapshot,
};
use std::collections::VecDeque;
use std::sync::Weak;

struct Confirmation {
    target: SessionIdentity,
    action: SessionSignal,
    label: String,
}

pub(super) struct SessionMonitorDialog {
    owner: String,
    source: Weak<dyn DatabaseDriver>,
    label: String,
    read_only: bool,
    monitor: Option<Arc<SessionMonitor>>,
    snapshot: SessionSnapshot,
    selected: Option<i32>,
    confirmation: Option<Confirmation>,
    pub(super) signaling: bool,
    task: Option<Task<()>>,
    message: String,
}

impl SessionMonitorDialog {
    fn selected_row(&self) -> Option<&SessionActivity> {
        let selected = self.selected?;
        self.snapshot.rows.iter().find(|row| row.pid == selected)
    }

    fn prepare_confirmation(&mut self, action: SessionSignal) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let Some(target) = signal_target(row, action, self.read_only) else {
            return;
        };
        self.confirmation = Some(Confirmation {
            target: target.clone(),
            action,
            label: format!(
                "PID {} · {} · {}",
                row.pid,
                row.role.as_deref().unwrap_or("Unknown role"),
                row.database.as_deref().unwrap_or("Unknown database")
            ),
        });
    }

    fn take_confirmation(&mut self, connections: &ConnectionManager) -> Option<Confirmation> {
        if !self.current(connections) || self.task.is_some() {
            return None;
        }
        self.confirmation.take()
    }

    fn current(&self, connections: &ConnectionManager) -> bool {
        connections
            .session(&self.owner)
            .driver
            .as_ref()
            .is_some_and(|driver| self.source.ptr_eq(&Arc::downgrade(driver)))
    }
}

fn signal_target(
    row: &SessionActivity,
    action: SessionSignal,
    read_only: bool,
) -> Option<&SessionIdentity> {
    row.identity.as_ref().filter(|identity| {
        !read_only && (action == SessionSignal::Terminate || identity.query_start.is_some())
    })
}

fn preserved_selection(old: Option<&SessionIdentity>, rows: &[SessionActivity]) -> Option<i32> {
    let old = old?;
    rows.iter().find_map(|row| {
        let current = row.identity.as_ref()?;
        (old.pid == current.pid && old.backend_start == current.backend_start).then_some(row.pid)
    })
}

const MAX_CHAIN_EDGES: usize = 128;

struct BlockingChain {
    edges: Vec<(i32, i32)>,
    incomplete: bool,
}

fn blocking_chain(rows: &HashMap<i32, &SessionActivity>, selected: i32) -> BlockingChain {
    let mut visited = HashSet::from([selected]);
    let mut queue = VecDeque::from([selected]);
    let mut chain = BlockingChain {
        edges: Vec::new(),
        incomplete: false,
    };
    while let Some(pid) = queue.pop_front() {
        let Some(row) = rows.get(&pid) else { continue };
        chain.incomplete |= row.blockers_truncated;
        for &blocker in &row.blockers {
            if chain.edges.len() == MAX_CHAIN_EDGES {
                chain.incomplete = true;
                return chain;
            }
            chain.edges.push((pid, blocker));
            if blocker != 0 && visited.insert(blocker) && rows.contains_key(&blocker) {
                queue.push_back(blocker);
            }
        }
    }
    chain
}

fn blocker_label(rows: &HashMap<i32, &SessionActivity>, blocker: i32) -> String {
    match rows.get(&blocker) {
        _ if blocker == 0 => "Prepared transaction".into(),
        Some(row) if row.state.is_some() => format!("PID {blocker}"),
        Some(_) => format!("PID {blocker} (wait visibility restricted)"),
        None => format!("PID {blocker} (details unavailable)"),
    }
}

impl Workspace {
    pub(super) fn can_open_session_monitor(&self, owner: &str) -> bool {
        let session = self.connections.session(owner);
        session.kind == DbDriverKind::Postgres
            && session.driver.is_some()
            && session.connection_config.is_some()
    }

    pub(super) fn open_session_monitor(&mut self, owner: &str, cx: &mut Context<Self>) {
        if !self.can_open_session_monitor(owner) || self.session_monitor.is_some() {
            return;
        }
        let session = self.connections.session(owner);
        let (Some(source), Some(config)) = (&session.driver, &session.connection_config) else {
            return;
        };
        let mut dialog = SessionMonitorDialog {
            owner: owner.to_owned(),
            source: Arc::downgrade(source),
            label: format!("{} · {}:{}", config.database, config.host, config.port),
            read_only: config.read_only,
            monitor: None,
            snapshot: SessionSnapshot::default(),
            selected: None,
            confirmation: None,
            signaling: false,
            task: None,
            message: "Connecting…".into(),
        };
        let config = config.clone();
        dialog.task = Some(cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move {
                let monitor = Arc::new(SessionMonitor::connect(config).await?);
                let snapshot = monitor.snapshot().await;
                Ok((monitor, snapshot))
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                let Some(dialog) = this.session_monitor.as_mut() else {
                    return;
                };
                if !dialog.current(&this.connections) {
                    dialog.message = "Connection changed. Reopen Session Monitor.".into();
                } else {
                    match result {
                        Ok((monitor, snapshot)) => {
                            dialog.monitor = Some(monitor);
                            finish_snapshot(dialog, snapshot, String::new());
                        }
                        Err(error) => dialog.message = format!("Could not connect: {error}"),
                    }
                }
                dialog.task = None;
                cx.notify();
            });
        }));
        self.session_monitor = Some(dialog);
        cx.notify();
    }

    fn select_monitor_session(&mut self, pid: i32, cx: &mut Context<Self>) {
        if let Some(dialog) = &mut self.session_monitor
            && dialog.current(&self.connections)
            && dialog.task.is_none()
            && dialog.confirmation.is_none()
        {
            dialog.selected = Some(pid);
            cx.notify();
        }
    }

    fn confirm_monitor_signal(&mut self, action: SessionSignal, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.session_monitor else {
            return;
        };
        if !dialog.current(&self.connections)
            || dialog.task.is_some()
            || dialog.confirmation.is_some()
        {
            return;
        }
        dialog.prepare_confirmation(action);
        cx.notify();
    }

    fn update_session_monitor(&mut self, submit: bool, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.session_monitor else {
            return;
        };
        if !dialog.current(&self.connections)
            || dialog.task.is_some()
            || submit != dialog.confirmation.is_some()
        {
            return;
        }
        let Some(monitor) = dialog.monitor.clone() else {
            return;
        };
        let confirmation = if submit {
            let Some(confirmation) = dialog.take_confirmation(&self.connections) else {
                return;
            };
            Some(confirmation)
        } else {
            None
        };
        dialog.signaling = confirmation.is_some();
        dialog.message = if submit {
            "Sending signal…"
        } else {
            "Refreshing…"
        }
        .into();
        dialog.task = Some(cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move {
                let message = if let Some(confirmation) = confirmation {
                    let action = confirmation.action;
                    let pid = confirmation.target.pid;
                    match monitor.signal(&confirmation.target, action).await {
                        Ok(()) => match action {
                            SessionSignal::Cancel => {
                                format!("Cancellation signal sent to PID {pid}.")
                            }
                            SessionSignal::Terminate => {
                                format!("Termination signal sent to PID {pid}.")
                            }
                        },
                        Err(error) => format!("Session action failed: {error}"),
                    }
                } else {
                    String::new()
                };
                Ok((monitor.snapshot().await, message))
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                let Some(dialog) = this.session_monitor.as_mut() else {
                    return;
                };
                dialog.task = None;
                dialog.signaling = false;
                if !dialog.current(&this.connections) {
                    dialog.monitor = None;
                    dialog.snapshot = SessionSnapshot::default();
                    dialog.selected = None;
                    dialog.message = "Connection changed. Reopen Session Monitor.".into();
                } else {
                    match result {
                        Ok((snapshot, message)) => finish_snapshot(dialog, snapshot, message),
                        Err(error) => finish_snapshot(dialog, Err(error), String::new()),
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn dismiss_session_monitor(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.session_monitor else {
            return;
        };
        if dialog.signaling {
            return;
        }
        if dialog.confirmation.take().is_none() {
            self.session_monitor = None;
        }
        cx.notify();
    }

    pub(super) fn session_monitor_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if key.key == "escape"
            || ((key.modifiers.platform || key.modifiers.control)
                && key.key.eq_ignore_ascii_case("w"))
        {
            self.dismiss_session_monitor(cx);
        }
    }

    pub(super) fn render_session_monitor(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dialog = self.session_monitor.as_ref().unwrap();
        let theme = &self.theme;
        let current = dialog.current(&self.connections);
        let interactive = current && dialog.task.is_none() && dialog.confirmation.is_none();
        let mut list = div()
            .id("session-monitor-rows")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll();
        if current {
            for row in &dialog.snapshot.rows {
                let pid = row.pid;
                let mut item = div()
                    .id(("monitor-session", pid as usize))
                    .p_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(format!(
                        "{} · {} · {}",
                        pid,
                        row.role.as_deref().unwrap_or("Unknown role"),
                        row.database.as_deref().unwrap_or("Unknown database")
                    ))
                    .child(div().text_color(theme.text_muted).child(format!(
                        "{}{}",
                        row.state.as_deref().unwrap_or("Details unavailable"),
                        if row.blockers.is_empty() {
                            ""
                        } else {
                            " · Blocked"
                        }
                    )));
                if dialog.selected == Some(pid) {
                    item = item.bg(theme.bg_surface_selected);
                }
                if interactive {
                    item = item
                        .cursor_pointer()
                        .hover(|item| item.bg(theme.bg_surface_hover))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select_monitor_session(pid, cx)),
                        );
                }
                list = list.child(item);
            }
        }

        let selection = current.then(|| dialog.selected_row()).flatten();
        let mut details = div()
            .id("session-monitor-details")
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_2();
        if let Some(row) = selection {
            let wait = match (&row.wait_type, &row.wait_event, &row.state) {
                (Some(kind), Some(event), _) => format!("{kind} · {event}"),
                (_, _, None) => "Unavailable".into(),
                _ => "None reported".into(),
            };
            details = details.child(format!("Wait · {wait}"));
            if !row.application.is_empty() {
                details = details.child(format!("Application · {}", row.application));
            }
            let by_pid: HashMap<_, _> = dialog
                .snapshot
                .rows
                .iter()
                .map(|row| (row.pid, row))
                .collect();
            let chain = blocking_chain(&by_pid, row.pid);
            if !chain.edges.is_empty() {
                details = details.child(
                    div()
                        .mt_2()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Blocking chain"),
                );
                for (waiter, blocker) in chain.edges {
                    details = details.child(format!(
                        "PID {waiter} waits for {}",
                        blocker_label(&by_pid, blocker)
                    ));
                }
            }
            if chain.incomplete {
                details = details.child(
                    div()
                        .text_color(theme.text_warning)
                        .child("More blockers exist than this view can display."),
                );
            }
            if let Some(active_ms) = row.active_ms {
                details = details.child(
                    div()
                        .mt_2()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Active query · {active_ms} ms")),
                );
            }
            if let Some(query) = &row.active_query {
                details = details.child(
                    div()
                        .mt_1()
                        .p_2()
                        .bg(theme.bg_surface_hover)
                        .font_family("Menlo")
                        .whitespace_normal()
                        .child(query.clone()),
                );
            }
        } else {
            details = details.child(
                div()
                    .text_color(theme.text_muted)
                    .child("Select a session to inspect its activity."),
            );
        }
        let refresh = tool("Refresh");
        let close = tool("Close");
        let mut actions =
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(if interactive && dialog.monitor.is_some() {
                    refresh.on_click(
                        cx.listener(|this, _, _, cx| this.update_session_monitor(false, cx)),
                    )
                } else {
                    refresh.opacity(0.5)
                });
        if let Some(row) = selection {
            for (label, action) in [
                ("Cancel query…", SessionSignal::Cancel),
                ("Terminate session…", SessionSignal::Terminate),
            ] {
                let button = tool(label);
                actions = actions.child(
                    if interactive && signal_target(row, action, dialog.read_only).is_some() {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.confirm_monitor_signal(action, cx)
                        }))
                    } else {
                        button.opacity(0.5)
                    },
                );
            }
        }
        actions = actions.child(if dialog.signaling || dialog.confirmation.is_some() {
            close.opacity(0.5)
        } else {
            close.on_click(cx.listener(|this, _, _, cx| this.dismiss_session_monitor(cx)))
        });
        let mut panel = div()
            .w(px(840.))
            .h(px(560.))
            .max_w_full()
            .max_h_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .text_color(theme.text_primary)
            .child(div().font_weight(FontWeight::BOLD).child("Session Monitor"))
            .child(
                div()
                    .text_color(theme.text_muted)
                    .child(dialog.label.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .w(relative(0.33))
                            .min_w(px(0.))
                            .border_r_1()
                            .border_color(theme.border)
                            .child(list),
                    )
                    .child(details),
            );
        if dialog.read_only {
            panel = panel.child(
                div()
                    .text_color(theme.text_muted)
                    .child("Session actions are disabled on read-only connections."),
            );
        }
        if current && dialog.snapshot.truncated {
            panel = panel.child(
                div()
                    .text_color(theme.text_warning)
                    .child("Showing the first 256 sessions, with lock waiters first."),
            );
        }
        let message = if current {
            dialog.message.as_str()
        } else {
            "Connection changed. Reopen Session Monitor."
        };
        if !message.is_empty() {
            panel = panel.child(div().child(message.to_owned()));
        }
        if let Some(confirmation) = &dialog.confirmation {
            let (title, hint, button) = match confirmation.action {
                SessionSignal::Cancel => (
                    "Cancel the selected query?",
                    "Its session will stay connected.",
                    "Cancel query",
                ),
                SessionSignal::Terminate => (
                    "Terminate the selected session?",
                    "The session will disconnect and any uncommitted work will be rolled back.",
                    "Terminate session",
                ),
            };
            panel = panel.child(
                div()
                    .p_2()
                    .border_1()
                    .border_color(theme.text_warning)
                    .child(div().font_weight(FontWeight::BOLD).child(title))
                    .child(confirmation.label.clone())
                    .child(hint)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(tool("Go back").on_click(
                                cx.listener(|this, _, _, cx| this.dismiss_session_monitor(cx)),
                            ))
                            .child(if current {
                                tool(button).on_click(cx.listener(|this, _, _, cx| {
                                    this.update_session_monitor(true, cx)
                                }))
                            } else {
                                tool(button).opacity(0.5)
                            }),
                    ),
            );
        } else {
            panel = panel.child(actions);
        }
        div()
            .absolute()
            .inset_0()
            .p_3()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(panel)
    }
}

fn finish_snapshot(
    dialog: &mut SessionMonitorDialog,
    snapshot: anyhow::Result<SessionSnapshot>,
    mut message: String,
) {
    match snapshot {
        Ok(snapshot) => {
            dialog.selected = preserved_selection(
                dialog.selected_row().and_then(|row| row.identity.as_ref()),
                &snapshot.rows,
            );
            if snapshot.rows.is_empty() {
                if !message.is_empty() {
                    message.push(' ');
                }
                message.push_str("No PostgreSQL client sessions found.");
            }
            dialog.snapshot = snapshot;
        }
        Err(error) => {
            dialog.selected = None;
            dialog.snapshot = SessionSnapshot::default();
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(&format!("Could not refresh sessions: {error}"));
        }
    }
    dialog.message = message;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use core::prelude::v1::test;

    fn activity(pid: i32, blockers: Vec<i32>) -> SessionActivity {
        SessionActivity {
            pid,
            database: Some("db".into()),
            role: Some("role".into()),
            application: String::new(),
            state: Some("active".into()),
            wait_type: Some("Lock".into()),
            wait_event: None,
            identity: Some(SessionIdentity {
                pid,
                backend_start: DateTime::UNIX_EPOCH,
                query_start: Some(DateTime::UNIX_EPOCH),
            }),
            active_query: None,
            active_ms: Some(0),
            blockers,
            blockers_truncated: false,
        }
    }

    #[test]
    fn session_monitor_lists_each_wait_edge_once_including_cycles_and_unlisted_targets() {
        let rows = [
            activity(1, vec![2]),
            activity(2, vec![1, 0, 9]),
            activity(3, vec![9]),
        ];
        let rows = rows.iter().map(|row| (row.pid, row)).collect();
        let chain = blocking_chain(&rows, 1);
        assert_eq!(chain.edges, vec![(1, 2), (2, 1), (2, 0), (2, 9)]);
        assert!(!chain.incomplete);
        assert_eq!(blocker_label(&rows, 0), "Prepared transaction");
        assert_eq!(blocker_label(&rows, 9), "PID 9 (details unavailable)");
    }

    #[test]
    fn session_monitor_bounds_visible_blocking_edges_and_reports_omissions() {
        let first = activity(1, (2..=65).collect());
        let second = activity(2, (66..=129).collect());
        let third = activity(3, vec![130]);
        let rows = HashMap::from([(1, &first), (2, &second), (3, &third)]);
        let chain = blocking_chain(&rows, 1);
        assert_eq!(chain.edges.len(), MAX_CHAIN_EDGES);
        assert!(chain.incomplete);
        let mut limited = activity(1, vec![2]);
        limited.blockers_truncated = true;
        assert!(blocking_chain(&HashMap::from([(1, &limited)]), 1).incomplete);
    }

    #[test]
    fn session_monitor_signal_requires_identity_and_cancel_requires_an_active_query() {
        let mut row = activity(1, vec![]);
        assert!(signal_target(&row, SessionSignal::Cancel, false).is_some());
        assert!(signal_target(&row, SessionSignal::Cancel, true).is_none());
        assert!(signal_target(&row, SessionSignal::Terminate, true).is_none());
        row.identity.as_mut().unwrap().query_start = None;
        assert!(signal_target(&row, SessionSignal::Cancel, false).is_none());
        assert!(signal_target(&row, SessionSignal::Terminate, false).is_some());
        row.identity = None;
        assert!(signal_target(&row, SessionSignal::Terminate, false).is_none());
    }

    #[test]
    fn session_monitor_confirmation_keeps_its_target_and_rejects_a_replaced_driver() {
        let original: Arc<dyn DatabaseDriver> = Arc::new(db_wire::SqliteDriver::new_memory());
        let mut connections = ConnectionManager::new(vec![], None);
        connections.session_mut("profile").driver = Some(original.clone());
        let mut dialog = SessionMonitorDialog {
            owner: "profile".into(),
            source: Arc::downgrade(&original),
            label: String::new(),
            read_only: false,
            monitor: None,
            snapshot: SessionSnapshot {
                rows: vec![activity(1, vec![]), activity(2, vec![])],
                truncated: false,
            },
            selected: Some(1),
            confirmation: None,
            signaling: false,
            task: None,
            message: String::new(),
        };
        dialog.prepare_confirmation(SessionSignal::Cancel);
        dialog.selected = Some(2);
        let confirmed = dialog.take_confirmation(&connections).unwrap();
        assert_eq!(confirmed.target.pid, 1);
        assert_eq!(confirmed.action, SessionSignal::Cancel);
        assert!(dialog.take_confirmation(&connections).is_none());
        dialog.prepare_confirmation(SessionSignal::Terminate);
        connections.session_mut("profile").driver =
            Some(Arc::new(db_wire::SqliteDriver::new_memory()));
        assert!(dialog.take_confirmation(&connections).is_none());
    }

    #[test]
    fn session_monitor_refresh_preserves_selection_only_for_the_same_backend() {
        let old = activity(1, vec![]).identity.unwrap();
        let mut row = activity(1, vec![]);
        row.identity.as_mut().unwrap().query_start = None;
        assert_eq!(
            preserved_selection(Some(&old), std::slice::from_ref(&row)),
            Some(1)
        );
        row.identity.as_mut().unwrap().backend_start += chrono::Duration::seconds(1);
        assert_eq!(preserved_selection(Some(&old), &[row]), None);
    }
}
