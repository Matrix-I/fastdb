use super::script_tools::tool;
use super::*;

pub(super) type ColumnMap = HashMap<(String, String), Vec<ColumnMeta>>;
type Tree = BTreeMap<String, Vec<TableSummary>>;
const RESULT_LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Hit {
    schema: String,
    table: String,
    column: Option<String>,
    kind: String,
}

fn display_part(part: &str) -> std::borrow::Cow<'_, str> {
    if part.contains(['.', '"']) {
        format!("{part:?}").into()
    } else {
        part.into()
    }
}

fn matches(tree: &Tree, columns: &ColumnMap, query: &str) -> Vec<Hit> {
    let needle = query.trim().to_lowercase();
    let mut hits = Vec::new();
    if needle.is_empty() {
        return hits;
    }
    for (schema, relations) in tree {
        let schema_matches = schema.to_lowercase().contains(&needle);
        for relation in relations {
            let table = &relation.name;
            let path = needle.contains('.').then(|| {
                format!("{}.{}", display_part(schema), display_part(table)).to_lowercase()
            });
            if schema_matches
                || table.to_lowercase().contains(&needle)
                || path.as_ref().is_some_and(|p| p.contains(&needle))
            {
                hits.push(Hit {
                    schema: schema.clone(),
                    table: table.clone(),
                    column: None,
                    kind: relation.table_type.clone(),
                });
                if hits.len() > RESULT_LIMIT {
                    return hits;
                }
            }
            if let Some(fields) = columns.get(&(schema.clone(), table.clone())) {
                for field in fields.iter().filter(|c| {
                    c.name.to_lowercase().contains(&needle)
                        || path.as_ref().is_some_and(|p| {
                            format!("{p}.{}", display_part(&c.name).to_lowercase())
                                .contains(&needle)
                        })
                }) {
                    hits.push(Hit {
                        schema: schema.clone(),
                        table: table.clone(),
                        column: Some(field.name.clone()),
                        kind: "Column".into(),
                    });
                    if hits.len() > RESULT_LIMIT {
                        return hits;
                    }
                }
            }
        }
    }
    hits
}

pub(super) struct ObjectSearch {
    owner: String,
    tree: Arc<Tree>,
    text: String,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    hits: Vec<Hit>,
    message: String,
    task: Option<Task<()>>,
}

impl Workspace {
    pub(super) fn open_object_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(owner) = self.connections.active_connection_id.clone() else {
            return;
        };
        self.object_search = Some(ObjectSearch {
            owner,
            tree: self.connections.active().schema_tree.clone(),
            text: query.to_owned(),
            caret: query.chars().count(),
            anchor: None,
            history: LineHistory::new(),
            hits: Vec::new(),
            message: "Press Enter to search loaded relations and columns.".into(),
            task: None,
        });
        cx.notify();
    }

    fn run_object_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.object_search else {
            return;
        };
        if search.task.is_some() {
            return;
        }
        let session = self.connections.active();
        if self.connections.active_connection_id.as_ref() != Some(&search.owner)
            || session.driver.is_none()
        {
            search.message = "Connection changed. Reopen search.".into();
            cx.notify();
            return;
        }
        search.tree = session.schema_tree.clone();
        let tree = search.tree.clone();
        let columns = session.table_columns.clone();
        let query = search.text.clone();
        search.hits.clear();
        if query.trim().is_empty() {
            search.message = "Enter a schema, relation or column name.".into();
            cx.notify();
            return;
        }
        search.message = "Searching loaded metadata…".into();
        search.task = Some(cx.spawn(async move |this, cx| {
            let mut hits = cx
                .background_executor()
                .spawn(async move { matches(&tree, &columns, &query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(search) = &mut this.object_search else {
                    return;
                };
                search.message = if hits.len() > RESULT_LIMIT {
                    hits.truncate(RESULT_LIMIT);
                    format!("First {RESULT_LIMIT} matches. Refine your search for more.")
                } else if hits.is_empty() {
                    "No matches in loaded metadata.".into()
                } else {
                    format!("{} matches in loaded metadata.", hits.len())
                };
                search.hits = hits;
                search.task = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn choose_object(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(search) = &mut self.object_search else {
            return;
        };
        let session = self.connections.active();
        if self.connections.active_connection_id.as_ref() != Some(&search.owner)
            || session.driver.is_none()
            || !Arc::ptr_eq(&session.schema_tree, &search.tree)
        {
            search.hits.clear();
            search.message = "Metadata changed. Search again.".into();
            cx.notify();
            return;
        }
        let Some(hit) = search.hits.get(index).cloned() else {
            return;
        };
        if let Some(column) = hit.column {
            let title = format!("{}.{}", display_part(&hit.schema), display_part(&hit.table));
            search.message = session
                .properties
                .get(&(hit.schema, hit.table))
                .and_then(|properties| properties.selected_column_details(&column))
                .map(|details| format!("{title}\n{details}"))
                .unwrap_or_else(|| "Column details unavailable. Search again.".into());
        } else {
            let owner = search.owner.clone();
            self.object_search = None;
            self.query_table(&owner, &hit.schema, &hit.table, cx);
        }
        cx.notify();
    }

    pub(super) fn object_search_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(search) = &mut self.object_search else {
            return;
        };
        match key.key.as_str() {
            "escape" => self.object_search = None,
            "w" if key.modifiers.platform || key.modifiers.control => self.object_search = None,
            "enter" | "return" => self.run_object_search(cx),
            _ => {
                let before = search.text.clone();
                handle_line_key(
                    LineInput {
                        text: &mut search.text,
                        caret: &mut search.caret,
                        anchor: &mut search.anchor,
                        history: &mut search.history,
                        digits_only: false,
                    },
                    key,
                    cx,
                );
                if before != search.text {
                    search.task = None;
                    search.hits.clear();
                    search.message = "Press Enter to search.".into();
                }
            }
        }
        cx.notify();
    }

    pub(super) fn render_object_search(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let search = self.object_search.as_ref().unwrap();
        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(s) = &mut this.object_search {
                    apply_pick(&mut s.caret, &mut s.anchor, index, kind, true);
                    cx.notify();
                }
            });
        });
        let mut rows = div()
            .id("object-search-results")
            .max_h(px(300.))
            .overflow_y_scroll();
        for (i, hit) in search.hits.iter().enumerate() {
            let mut name = format!("{}.{}", display_part(&hit.schema), display_part(&hit.table));
            if let Some(column) = &hit.column {
                name.push('.');
                name.push_str(&display_part(column));
            }
            rows = rows.child(
                div()
                    .id(("object-hit", i))
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .hover({
                        let hover = self.theme.bg_surface_hover;
                        move |s| s.bg(hover)
                    })
                    .child(format!("{}   {name}", hit.kind))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose_object(i, cx))),
            );
        }
        let input = ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
            .p_2()
            .border_1()
            .border_color(self.theme.border_focused)
            .child(render_field_content(
                &search.text,
                "Name contains…",
                true,
                search.anchor,
                search.caret,
                true,
                &self.theme,
                &pick,
            ));
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .occlude()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .w(px(520.))
                    .rounded_md()
                    .bg(self.theme.bg_surface)
                    .child("Search loaded objects")
                    .child(input)
                    .child(
                        div()
                            .id("object-search-details")
                            .max_h(px(150.))
                            .overflow_y_scroll()
                            .child(search.message.clone()),
                    )
                    .child(rows)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                tool("Search").on_click(
                                    cx.listener(|this, _, _, cx| this.run_object_search(cx)),
                                ),
                            )
                            .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                                this.object_search = None;
                                cx.notify();
                            }))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{ColumnMap, RESULT_LIMIT, TableSummary, Tree, matches};
    use db_wire::ColumnMeta;
    use mem_engine::DataType;

    fn relation(schema: &str, name: &str) -> TableSummary {
        TableSummary {
            schema: schema.into(),
            name: name.into(),
            table_type: "VIEW".into(),
            estimated_rows: None,
        }
    }

    #[test]
    fn search_preserves_relation_and_column_identity_and_treats_input_literally() {
        let tree = Tree::from([
            ("one".into(), vec![relation("one", "Reports")]),
            ("two".into(), vec![relation("two", "Reports")]),
        ]);
        let cols = ColumnMap::from([(
            ("two".into(), "Reports".into()),
            vec![ColumnMeta::new("Tỷ_lệ%", DataType::Text)],
        )]);
        let relations = matches(&tree, &cols, " REPORT ");
        assert_eq!(
            relations
                .iter()
                .map(|h| (h.schema.as_str(), h.kind.as_str()))
                .collect::<Vec<_>>(),
            [("one", "VIEW"), ("two", "VIEW")]
        );
        assert_eq!(matches(&tree, &cols, "Two")[0].table, "Reports");
        let hits = matches(&tree, &cols, "Ỷ_LỆ%");
        assert_eq!(hits.len(), 1);
        assert_eq!(matches(&tree, &cols, "two.reports.tỷ_lệ%"), hits);
        assert_eq!(
            (&*hits[0].schema, &*hits[0].table, hits[0].column.as_deref()),
            ("two", "Reports", Some("Tỷ_lệ%"))
        );
        assert!(matches(&tree, &cols, "   ").is_empty());
        assert!(matches(&tree, &cols, "absent").is_empty());
    }

    #[test]
    fn collects_only_one_overflow_hit() {
        let tree = Tree::from([(
            "s".into(),
            (0..200)
                .map(|i| relation("s", &format!("view{i}")))
                .collect(),
        )]);
        assert_eq!(
            matches(&tree, &ColumnMap::new(), "view").len(),
            RESULT_LIMIT + 1
        );
    }
}
