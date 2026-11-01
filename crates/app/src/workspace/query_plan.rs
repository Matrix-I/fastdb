//! Script query plan actions and tree view.
use super::*;

impl Workspace {
    pub(super) fn explain_active_statement(&mut self, analyze: bool, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        if let Some(tab) = self
            .tabs
            .get(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        {
            self.save_then(vec![tab.id], AfterSave::Explain(tab.id, analyze), cx);
        }
    }

    pub(super) fn explain_saved_statement(
        &mut self,
        id: u64,
        analyze: bool,
        cx: &mut Context<Self>,
    ) {
        if self.operations.is_running() {
            return;
        }
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let kind = self.tab_provider(index);
        let Some(tab) = self
            .tabs
            .get_mut(index)
            .filter(|t| t.is_script() && !t.file_busy)
        else {
            return;
        };
        let statement = tab.editor.active_statement();
        match db_wire::query_plan::explain_sql(kind, &statement, analyze) {
            Ok(sql) => {
                self.tabs[index].pending_statements.clear();
                self.tabs[index].new_script_result(sql.clone());
                self.start_query(index, sql, false, false, cx);
                if self
                    .operations
                    .active()
                    .is_some_and(|op| op.tab_id == self.tabs[index].id)
                {
                    self.tabs[index].plan_requested = true;
                }
            }
            Err(error) => self.tabs[index].last_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn render_query_plan(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = &self.theme;
        let tab = &self.tabs[index];
        let tab_id = tab.id;
        let plan = tab.query_plan.as_ref().expect("plan view requires a plan");
        let mut tree = div().id("query-plan-tree").flex().flex_col().flex_1().min_h_0()
            .overflow_y_scroll().p_3().gap_2().text_size(px(12.0)).text_color(theme.text_primary)
            .child(div().text_color(theme.text_secondary).child(plan.summary.clone()))
            .child(div().text_color(theme.text_muted).child("Bars show inclusive estimated cost relative to the root; actual node times and rows are per loop."));
        for i in plan.visible_indices() {
            let node = &plan.nodes[i];
            let marker = if node.has_children {
                if node.collapsed { "▸" } else { "▾" }
            } else {
                "·"
            };
            tree = tree.child(
                div()
                    .id(("plan-node", i))
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap_1()
                    .ml(px(node.depth.min(32) as f32 * 18.0))
                    .p_2()
                    .border_l_2()
                    .border_color(theme.border)
                    .bg(theme.bg_surface)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(plan) = this
                            .tabs
                            .iter_mut()
                            .find(|t| t.id == tab_id)
                            .and_then(|t| t.query_plan.as_mut())
                            && let Some(node) = plan.nodes.get_mut(i)
                        {
                            node.collapsed = !node.collapsed;
                        }
                        cx.notify();
                    }))
                    .child(format!("{marker} {}", node.title))
                    .child(
                        div()
                            .h(px(3.0))
                            .w(relative(node.cost_fraction))
                            .bg(theme.text_accent),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(node.details.clone()),
                    ),
            );
        }
        tree
    }
}
