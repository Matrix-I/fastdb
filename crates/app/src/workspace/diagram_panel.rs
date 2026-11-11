mod svg;

use super::diagram_model::{
    CARD_PADDING, DiagramModel, HEADER, HEADER_FONT, NAME_FONT, ROW, single_line_label,
};
use super::script_tools::tool;
use super::*;

impl Workspace {
    fn export_diagram_svg(&mut self, id: u64, graph: Arc<DiagramModel>, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if tab.export.busy || graph.nodes.is_empty() {
            return;
        }
        tab.export.busy = true;
        tab.export.message = Some("Choose a diagram export destination…".into());
        let dialog =
            cx.prompt_for_new_path(&platform_hal::paths::downloads_dir(), Some("diagram.svg"));
        cx.spawn(async move |this, cx| {
            let message = match dialog.await {
                Ok(Ok(Some(path))) => Some(
                    cx.background_executor()
                        .spawn(async move {
                            match svg::write_svg(&path, &graph) {
                                Ok(()) => format!("Exported diagram to {}", path.display()),
                                Err(error) => format!("Diagram export failed: {error}"),
                            }
                        })
                        .await,
                ),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => Some(format!("Cannot open export dialog: {error}")),
                Err(_) => Some("Export dialog closed unexpectedly".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.export.busy = false;
                    tab.export.message = message;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_diagram_drag(&mut self, id: u64, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
            && tab.diagram.drag.is_some()
        {
            tab.diagram.move_card(pointer);
            tab.diagram.finish_drag(pointer);
            cx.notify();
        }
    }

    pub(super) fn render_diagram_panel(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let base = self.tab_session(index).diagram.clone();
        let owner = self.tabs[index]
            .connection_id
            .as_deref()
            .filter(|id| {
                self.connections
                    .saved_connections
                    .iter()
                    .any(|connection| connection.id == *id)
            })
            .map(str::to_owned);
        let tab = &mut self.tabs[index];
        let schema = tab.schema_name.as_deref().unwrap_or_default();
        let table = tab.table_name.as_deref().unwrap_or_default();
        tab.diagram.set_layout_path(owner.as_deref().map(|owner| {
            super::diagram_positions::layout_path(
                &platform_hal::paths::config_dir(),
                owner,
                schema,
                table,
            )
        }));
        let style = window.text_style();
        let graph = tab.diagram.graph(
            base,
            tab.schema_name.as_deref().unwrap_or_default(),
            tab.table_name.as_deref().unwrap_or_default(),
            |text, size, bold| {
                let mut font = style.font();
                if bold {
                    font.weight = FontWeight::BOLD;
                }
                let run = gpui::TextRun {
                    len: text.len(),
                    font,
                    color: style.color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                f32::from(
                    window
                        .text_system()
                        .shape_line(text.to_owned().into(), px(size), &[run], None)
                        .width,
                )
            },
        );
        let fit_graph = graph.clone();
        let focus_graph = graph.clone();
        let export_graph = graph.clone();
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
                    let graph = fit_graph.clone();
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
                        let graph = focus_graph.clone();
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
        let toolbar = if graph.nodes.is_empty() || tab.export.busy {
            toolbar
        } else {
            toolbar.child(
                tool("Export SVG…").on_click(cx.listener(move |this, _, _, cx| {
                    this.export_diagram_svg(id, export_graph.clone(), cx);
                })),
            )
        };
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
                            if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                                let centered =
                                    tab.diagram.center_initial_view(&line_graph, viewport.size);
                                if centered || tab.diagram.viewport != viewport.size {
                                    tab.diagram.viewport = viewport.size;
                                    this.notify_result(cx);
                                }
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
        // Hit regions follow the same orthogonal segments as the painted links.
        for edge in 0..graph.edges.len() {
            for (segment, pair) in graph.route(edge).windows(2).enumerate() {
                let (x1, y1) = pair[0];
                let (x2, y2) = pair[1];
                scene = scene.child(
                    div()
                        .id(("er-edge-segment", edge * 5 + segment))
                        .absolute()
                        .left(px(x1.min(x2) * zoom - 4.0))
                        .top(px(y1.min(y2) * zoom - 4.0))
                        .w(px((x2 - x1).abs() * zoom + 8.0))
                        .h(px((y2 - y1).abs() * zoom + 8.0))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                                tab.diagram.selected_edge = Some(edge);
                                tab.diagram.selected_column = None;
                            }
                            cx.notify();
                        })),
                );
            }
        }
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
            let (card_x, card_y) = (node.x, node.y);
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
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, _| {
                        if event.click_count == 1
                            && let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id)
                        {
                            tab.diagram.drag = Some((i, event.position, card_x, card_y));
                        }
                    }),
                )
                .child(
                    div()
                        .h(px(HEADER * zoom))
                        .px(px(CARD_PADDING * zoom))
                        .flex()
                        .items_center()
                        .bg(theme.bg_editor)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(HEADER_FONT * zoom))
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
                        .child(
                            single_line_label(&format!("{}.{}", node.schema, node.table))
                                .into_owned(),
                        ),
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
                        .px(px(CARD_PADDING * zoom))
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .text_size(px(NAME_FONT * zoom))
                        .bg(if graph.mapped_column(selected, i, column_index) {
                            Rgba {
                                a: 0.3,
                                ..theme.text_accent
                            }
                        } else {
                            theme.bg_surface
                        })
                        .text_color(if graph.mapped_column(selected, i, column_index) {
                            theme.text_accent
                        } else {
                            theme.text_primary
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .child(single_line_label(&column.name).into_owned()),
                        ),
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
                    if event.pressed_button == Some(MouseButton::Left) && tab.diagram.drag.is_some()
                    {
                        tab.diagram.move_card(event.position);
                        cx.notify();
                    }
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
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                    this.finish_diagram_drag(id, event.position, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                    this.finish_diagram_drag(id, event.position, cx);
                }),
            )
            .child(scene);
        let mut footer = selected.and_then(|i| graph.edges.get(i)).map(|e| {
            let f = &e.fk;
            format!("{}: {}.{}.{} → {}.{}.{}", f.name, f.source_schema, f.source_table,
                f.source_column, f.target_schema, f.target_table, f.target_column)
        }).unwrap_or_else(|| "FK arrows point to referenced columns · Scroll both axes · Cmd/Ctrl + scroll or +/− to zoom · Click a column for properties · Middle-drag to pan · Drag anywhere on a table to move · Double-click a table to open".into());
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
        if let Some(message) = &self.tabs[index].export.message {
            panel = panel.child(div().px_2().text_size(px(11.0)).child(message.clone()));
        }
        if let Some(error) = &self.tabs[index].diagram.layout_error {
            panel = panel.child(
                div()
                    .px_2()
                    .text_color(theme.text_warning)
                    .child(error.clone()),
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
    for i in 0..graph.edges.len() {
        let route = graph.route(i);
        let at = |(x, y)| origin + point(px(x * zoom), px(y * zoom));
        let mut path = PathBuilder::stroke(px(if selected == Some(i) { 3.0 } else { 2.0 }));
        path.move_to(at(route[0]));
        for &vertex in &route[1..] {
            path.line_to(at(vertex));
        }
        let (x2, y2) = route[route.len() - 1];
        let direction = (route[route.len() - 2].0 - x2).signum();
        path.move_to(at((x2 + direction * 10.0, y2 - 6.0)));
        path.line_to(at((x2, y2)));
        path.line_to(at((x2 + direction * 10.0, y2 + 6.0)));
        if let Ok(path) = path.build() {
            window.paint_path(path, if selected == Some(i) { accent } else { color });
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn drag_handler_covers_the_whole_table_card() {
        let source = include_str!("diagram_panel.rs");
        let card = source.split("let mut card = div()").nth(1).unwrap();
        let (card_handlers, children) = card.split_once(".child(").unwrap();
        assert!(card_handlers.contains(".on_mouse_down("));
        assert!(card_handlers.contains("tab.diagram.drag = Some("));
        let header = children.split("if node.columns.is_empty()").next().unwrap();
        assert!(header.contains("this.query_table("));
        assert!(!header.contains("tab.diagram.drag = Some("));
    }
}
