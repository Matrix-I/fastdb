use super::diagram_model::{DiagramModel, HEADER, ROW};
use super::script_tools::tool;
use super::*;

impl Workspace {
    pub(super) fn render_diagram_panel(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let graph = self.tab_session(index).diagram.clone();
        let theme = self.theme.clone();
        let tab = &self.tabs[index];
        let id = tab.id;
        let zoom = tab.diagram.zoom;
        let scroll = tab.diagram.scroll.clone();
        let selected = tab.diagram.selected_edge;
        let root = (tab.schema_name.clone(), tab.table_name.clone());
        let owner = tab.connection_id.clone();
        let toolbar =
            div()
                .id("diagram-toolbar")
                .overflow_x_scroll()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .p_2()
                .text_size(px(11.0))
                .text_color(theme.text_secondary)
                .child(format!(
                    "{} tables · {} FK column links",
                    graph.nodes.len(),
                    graph.edges.len()
                ))
                .child(tool("−").id("diagram-zoom-out").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                            tab.diagram.zoom_to(tab.diagram.zoom / 1.2);
                        }
                        cx.notify();
                    },
                )))
                .child(format!("{:.0}%", zoom * 100.0))
                .child(tool("+").id("diagram-zoom-in").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                            tab.diagram.zoom_to(tab.diagram.zoom * 1.2);
                        }
                        cx.notify();
                    },
                )))
                .child(tool("Fit").on_click(cx.listener(move |this, _, _, cx| {
                    let Some(index) = this.tabs.iter().position(|t| t.id == id) else {
                        return;
                    };
                    let graph = this.tab_session(index).diagram.clone();
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        let size = tab.diagram.scroll.bounds().size;
                        tab.diagram.zoom_to(
                            (f32::from(size.width) / graph.width.max(1.0))
                                .min(f32::from(size.height) / graph.height.max(1.0)),
                        );
                        tab.diagram.scroll.set_offset(point(px(0.0), px(0.0)));
                    }
                    cx.notify();
                })))
                .child(tool("Reset").on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        tab.diagram.zoom = 1.0;
                        tab.diagram.scroll.set_offset(point(px(0.0), px(0.0)));
                    }
                    cx.notify();
                })))
                .child(
                    tool("Focus table").on_click(cx.listener(move |this, _, _, cx| {
                        let Some(index) = this.tabs.iter().position(|t| t.id == id) else {
                            return;
                        };
                        let graph = this.tab_session(index).diagram.clone();
                        let tab = &mut this.tabs[index];
                        if let Some(node) = graph.nodes.iter().find(|n| {
                            Some(&n.schema) == tab.schema_name.as_ref()
                                && Some(&n.table) == tab.table_name.as_ref()
                        }) {
                            tab.diagram.scroll.set_offset(point(
                                px(-node.x * tab.diagram.zoom + 20.0),
                                px(-node.y * tab.diagram.zoom + 20.0),
                            ));
                        }
                        cx.notify();
                    })),
                );
        let viewport_owner = cx.entity().downgrade();
        let line_graph = graph.clone();
        let line_scroll = scroll.clone();
        let line_color = theme.text_muted;
        let accent = theme.text_accent;
        let mut scene = div()
            .relative()
            .flex_none()
            .w(px((graph.width * zoom).max(1.0)))
            .h(px((graph.height * zoom).max(1.0)))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let viewport = line_scroll.bounds();
                        // Layout becomes available after render; invalidate once on resize so
                        // card culling uses the actual viewport, including the initial frame.
                        let _ = viewport_owner.update(cx, |this, cx| {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id)
                                && tab.diagram.viewport != viewport.size
                            {
                                tab.diagram.viewport = viewport.size;
                                this.notify_result(cx);
                            }
                        });
                        window.with_content_mask(
                            Some(ContentMask { bounds: viewport }),
                            |window| {
                                paint_links(
                                    &line_graph,
                                    zoom,
                                    selected,
                                    bounds.origin,
                                    line_color,
                                    accent,
                                    window,
                                );
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            );
        let offset = scroll.offset();
        let size = scroll.bounds().size;
        let viewport_w = f32::from(size.width).max(1000.0);
        let viewport_h = f32::from(size.height).max(800.0);
        for (i, node) in graph.nodes.iter().enumerate() {
            if !graph.visible(
                i,
                -f32::from(offset.x) - 100.0,
                -f32::from(offset.y) - 100.0,
                viewport_w + 200.0,
                viewport_h + 200.0,
                zoom,
            ) {
                continue;
            }
            let schema = node.schema.clone();
            let table = node.table.clone();
            let owner = owner.clone();
            let is_root =
                root.0.as_ref() == Some(&node.schema) && root.1.as_ref() == Some(&node.table);
            let mut card = div()
                .id(("er-table", i))
                .absolute()
                .left(px(node.x * zoom))
                .top(px(node.y * zoom))
                .w(px(node.width * zoom))
                .h(px(node.height * zoom))
                .overflow_hidden()
                .rounded_sm()
                .border_1()
                .border_color(if is_root {
                    theme.text_accent
                } else {
                    theme.border
                })
                .bg(theme.bg_surface)
                .child(
                    div()
                        .h(px(HEADER * zoom))
                        .px(px(8.0 * zoom))
                        .flex()
                        .items_center()
                        .bg(theme.bg_editor)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(12.0 * zoom))
                        .text_color(theme.text_primary)
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                if event.click_count == 2
                                    && let Some(owner) = &owner
                                {
                                    this.query_table(owner, &schema, &table, cx);
                                }
                            }),
                        )
                        .child(format!("{}.{}", node.schema, node.table)),
                );
            if node.columns.is_empty() {
                card = card.child(
                    div()
                        .h(px(ROW * zoom))
                        .text_size(px(10.0 * zoom))
                        .text_color(theme.text_muted)
                        .child("Columns unavailable"),
                );
            }
            for (column_index, column) in node.columns.iter().enumerate() {
                let fk = node
                    .outgoing
                    .iter()
                    .any(|e| graph.edges[*e].fk.source_column == column.name);
                card = card.child(
                    div()
                        .id(("er-column", i * 65536 + column_index))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                                tab.diagram.selected_column = Some((i, column_index));
                                tab.diagram.selected_edge = None;
                            }
                            cx.notify();
                        }))
                        .h(px(ROW * zoom))
                        .px(px(8.0 * zoom))
                        .flex()
                        .items_center()
                        .gap(px(4.0 * zoom))
                        .overflow_hidden()
                        .text_size(px(11.0 * zoom))
                        .text_color(theme.text_primary)
                        .child(if column.is_primary_key && fk {
                            "PK/FK"
                        } else if column.is_primary_key {
                            "PK"
                        } else if fk {
                            "FK"
                        } else {
                            "·"
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .child(column.name.clone()),
                        )
                        .child(
                            div()
                                .text_color(theme.text_muted)
                                .text_size(px(9.0 * zoom))
                                .child(
                                    column
                                        .raw_type
                                        .clone()
                                        .unwrap_or_else(|| column.data_type.to_string()),
                                ),
                        ),
                );
            }
            for &edge in &node.outgoing {
                let fk = &graph.edges[edge].fk;
                card = card.child(
                    div()
                        .id(("er-link", edge))
                        .h(px(20.0 * zoom))
                        .px(px(8.0 * zoom))
                        .text_size(px(9.0 * zoom))
                        .truncate()
                        .text_color(if selected == Some(edge) {
                            theme.text_accent
                        } else {
                            theme.text_secondary
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                                tab.diagram.selected_edge = Some(edge);
                                tab.diagram.selected_column = None;
                            }
                            cx.notify();
                        }))
                        .child(format!(
                            "{} → {}.{}.{}",
                            fk.source_column, fk.target_schema, fk.target_table, fk.target_column
                        )),
                );
            }
            scene = scene.child(card);
        }
        let scroll_view = div()
            .id(("er-scroll", id))
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_scroll()
            .track_scroll(&scroll)
            .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                if event.modifiers.platform || event.modifiers.control {
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        let delta =
                            f32::from(event.delta.pixel_delta(px(20.0)).y).clamp(-100.0, 100.0);
                        tab.diagram.zoom_at(
                            tab.diagram.zoom * (delta * 0.01).exp(),
                            event.position - tab.diagram.scroll.bounds().origin,
                        );
                    }
                    cx.stop_propagation();
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        tab.diagram.pan = Some(event.position);
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                    if event.pressed_button == Some(MouseButton::Middle) {
                        if let Some(previous) = tab.diagram.pan.replace(event.position) {
                            tab.diagram.scroll.set_offset(
                                tab.diagram.scroll.offset() + event.position - previous,
                            );
                            cx.notify();
                        }
                    } else {
                        tab.diagram.pan = None;
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _, _, _| {
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        tab.diagram.pan = None;
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(move |this, _, _, _| {
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                        tab.diagram.pan = None;
                    }
                }),
            )
            .child(scene);
        let mut footer = selected.and_then(|i| graph.edges.get(i)).map(|e| {
            let f = &e.fk;
            format!("{}: {}.{}.{} → {}.{}.{}", f.name, f.source_schema, f.source_table,
                f.source_column, f.target_schema, f.target_table, f.target_column)
        }).unwrap_or_else(|| "FK arrows point to referenced columns · Scroll both axes · Cmd/Ctrl + scroll or +/− to zoom · Click a column for properties · Middle-drag to pan · Double-click a table to open".into());
        if let Some((node, column)) = self.tabs[index].diagram.selected_column
            && let Some(node) = graph.nodes.get(node)
            && let Some(c) = node.columns.get(column)
        {
            footer = format!(
                "{}.{}.{} · type: {} · nullable: {} · PK: {} · FK: {} · default: {} · collation: {} · comment: {}",
                node.schema,
                node.table,
                c.name,
                c.raw_type
                    .clone()
                    .unwrap_or_else(|| c.data_type.to_string()),
                c.nullable,
                c.is_primary_key,
                node.outgoing
                    .iter()
                    .any(|e| graph.edges[*e].fk.source_column == c.name),
                c.default_val.as_deref().unwrap_or("Unavailable"),
                c.collation.as_deref().unwrap_or("Unavailable"),
                c.comment.as_deref().unwrap_or("Unavailable")
            );
        }
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(theme.bg_editor)
            .child(toolbar);
        if let Some(notice) = &graph.notice {
            panel = panel.child(
                div()
                    .px_2()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(notice.clone()),
            );
        }
        if graph.nodes.is_empty() {
            panel = panel.child(
                div()
                    .p_4()
                    .text_color(theme.text_muted)
                    .child("No table metadata loaded. Connect or refresh the schema."),
            );
        }
        panel
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(scroll_view)
                    .child(properties::grid::detail_scrollbar(
                        scroll.clone(),
                        true,
                        &theme,
                    )),
            )
            .child(properties::grid::detail_scrollbar(scroll, false, &theme))
            .child(
                div()
                    .flex_none()
                    .p_2()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(footer),
            )
    }
}

fn paint_links(
    graph: &DiagramModel,
    zoom: f32,
    selected: Option<usize>,
    origin: Point<Pixels>,
    color: Rgba,
    accent: Rgba,
    window: &mut Window,
) {
    for (i, edge) in graph.edges.iter().enumerate() {
        let source = &graph.nodes[edge.source];
        let target = &graph.nodes[edge.target];
        let y1 = source.y
            + HEADER
            + edge
                .source_row
                .map(|r| ROW * (r as f32 + 0.5))
                .unwrap_or(-HEADER / 2.0);
        let mut y2 = target.y
            + HEADER
            + edge
                .target_row
                .map(|r| ROW * (r as f32 + 0.5))
                .unwrap_or(-HEADER / 2.0);
        let x1 = source.x + source.width;
        let target_left = target.x > source.x;
        let x2 = target.x + if target_left { 0.0 } else { target.width };
        let lane = x1.max(if target_left { x1 } else { x2 }) + 20.0 + (i % 8) as f32 * 5.0;
        if edge.source == edge.target && (y1 - y2).abs() < 1.0 {
            y2 += 8.0;
        }
        let at = |x, y| origin + point(px(x * zoom), px(y * zoom));
        let mut path = PathBuilder::stroke(px(if selected == Some(i) { 2.5 } else { 1.3 }));
        path.move_to(at(x1, y1));
        path.line_to(at(lane, y1));
        path.line_to(at(lane, y2));
        path.line_to(at(x2, y2));
        let direction = if target_left { -1.0 } else { 1.0 };
        path.move_to(at(x2 + direction * 7.0, y2 - 4.0));
        path.line_to(at(x2, y2));
        path.line_to(at(x2 + direction * 7.0, y2 + 4.0));
        if let Ok(path) = path.build() {
            window.paint_path(path, if selected == Some(i) { accent } else { color });
        }
    }
}
