//! Rendering for the CSV import / table transfer modal.
use super::import_modal::{
    DESTINATION_LIMIT, ImportField, ImportModal, ImportSource, MappingPicker, TextField,
};
use super::script_tools::tool;
use super::transfer_mapping::{CONVERSIONS, conversion_label};
use super::*;
use db_wire::transfer::{csv::Encoding, import::CommitPolicy};
use gpui::prelude::FluentBuilder;

const PICKER_MAX_HEIGHT: f32 = 160.0;

fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let hover = theme.bg_surface_hover;
    let pill = div()
        .id(id)
        .px_1p5()
        .py_0p5()
        .rounded_xs()
        .border_1()
        .border_color(if selected {
            theme.border_focused
        } else {
            theme.border
        })
        .bg(if selected {
            theme.bg_surface_selected
        } else {
            theme.bg_surface
        })
        .text_color(if selected {
            theme.text_primary
        } else {
            theme.text_muted
        })
        .child(label.into());
    if enabled {
        pill.cursor_pointer().hover(move |s| s.bg(hover))
    } else {
        pill.opacity(0.5)
    }
}

fn caret_pick(field: ImportField, cx: &mut Context<Workspace>) -> CaretPick {
    let weak = cx.entity().downgrade();
    std::rc::Rc::new(move |index, kind, _, cx| {
        if let Some(app) = weak.upgrade() {
            app.update(cx, |this, cx| {
                if let Some(m) = &mut this.import_modal
                    && !m.is_running
                {
                    m.active_field = Some(field);
                    if let Some(text) = m.active_text() {
                        apply_pick(&mut text.caret, &mut text.anchor, index, kind, true);
                    }
                    cx.notify();
                }
            });
        }
    })
}

fn text_box(
    field: ImportField,
    value: &TextField,
    placeholder: &str,
    active: bool,
    theme: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let pick = caret_pick(field, cx);
    ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
        .flex_1()
        .px_2()
        .py_0p5()
        .border_1()
        .border_color(if active {
            theme.border_focused
        } else {
            theme.border
        })
        .rounded_xs()
        .bg(theme.bg_app)
        .child(render_field_content(
            &value.text,
            placeholder,
            active,
            if active { value.anchor } else { None },
            value.caret,
            true,
            theme,
            &pick,
        ))
}

impl Workspace {
    fn render_csv_options(&self, modal: &ImportModal, cx: &mut Context<Self>) -> Option<Div> {
        let ImportSource::Csv(csv) = &modal.source else {
            return None;
        };
        let theme = &self.theme;
        let enabled = !modal.is_running;
        let display = if csv.file_path.is_empty() {
            "No CSV file selected".to_string()
        } else {
            csv.file_path.clone()
        };
        let file_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .px_2()
                    .py_1()
                    .rounded_xs()
                    .bg(theme.bg_editor)
                    .border_1()
                    .border_color(theme.border)
                    .text_color(if csv.file_path.is_empty() {
                        theme.text_muted
                    } else {
                        theme.text_primary
                    })
                    .child(display),
            )
            .child(tool("Browse…").on_click(cx.listener(|this, _, _, cx| {
                if this.import_modal.as_ref().is_some_and(|m| m.is_running) {
                    return;
                }
                let dialog = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some("Select CSV File".into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = dialog.await
                        && let Some(path) = paths.into_iter().next()
                    {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(modal) = &mut this.import_modal
                                && !modal.is_running
                            {
                                modal.load_file(path.display().to_string());
                                cx.notify();
                            }
                        });
                    }
                })
                .detach();
            })));

        let mut format_row = div().flex().flex_wrap().items_center().gap_1();
        for (label, delim) in [
            ("Comma (,)", b','),
            ("Semicolon (;)", b';'),
            ("Tab (\\t)", b'\t'),
            ("Pipe (|)", b'|'),
        ] {
            let mut p = pill(label, label, csv.delimiter == delim, enabled, theme);
            if enabled {
                p = p.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal
                        && let ImportSource::Csv(csv) = &mut m.source
                    {
                        csv.delimiter = delim;
                        m.reload_preview();
                        cx.notify();
                    }
                }));
            }
            format_row = format_row.child(p);
        }
        let mut header = pill(
            "toggle-has-header",
            "Has Header",
            csv.has_header,
            enabled,
            theme,
        );
        if enabled {
            header = header.on_click(cx.listener(|this, _, _, cx| {
                if let Some(m) = &mut this.import_modal
                    && let ImportSource::Csv(csv) = &mut m.source
                {
                    csv.has_header = !csv.has_header;
                    m.reload_preview();
                    cx.notify();
                }
            }));
        }
        format_row = format_row.child(header);

        let mut encoding_row = div()
            .flex()
            .items_center()
            .gap_1()
            .child(div().text_color(theme.text_muted).child("Encoding"));
        for (label, encoding) in [
            ("UTF-8", Encoding::Utf8),
            ("UTF-16 LE", Encoding::Utf16Le),
            ("UTF-16 BE", Encoding::Utf16Be),
        ] {
            let mut p = pill(label, label, csv.encoding == encoding, enabled, theme);
            if enabled {
                p = p.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal
                        && let ImportSource::Csv(csv) = &mut m.source
                    {
                        csv.encoding = encoding;
                        m.reload_preview();
                        cx.notify();
                    }
                }));
            }
            encoding_row = encoding_row.child(p);
        }
        let null_active = modal.active_field == Some(ImportField::NullToken);
        let null_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_color(theme.text_muted).child("NULL token"))
            .child(text_box(
                ImportField::NullToken,
                &csv.null_token,
                "Empty: no NULL token",
                null_active,
                theme,
                cx,
            ))
            .child(
                div()
                    .text_color(theme.text_muted)
                    .child("Only unquoted fields match"),
            );

        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(file_row)
            .child(format_row)
            .child(encoding_row)
            .child(null_row);
        if !csv.preview_rows.is_empty() {
            section = section.child(self.render_csv_preview(modal, &csv.preview_rows));
        }
        Some(section)
    }

    fn render_csv_preview(&self, modal: &ImportModal, rows: &[Vec<String>]) -> Stateful<Div> {
        let theme = &self.theme;
        let cell = |text: String, bold: bool| {
            let c = div()
                .w(px(100.0))
                .flex_none()
                .px_1()
                .py_0p5()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(11.0))
                .child(text);
            if bold {
                c.font_weight(FontWeight::BOLD)
                    .text_color(theme.text_primary)
            } else {
                c.text_color(theme.text_muted)
            }
        };
        let mut header = div()
            .flex()
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border);
        for h in &modal.mapping.sources {
            header = header.child(cell(h.clone(), true));
        }
        let mut body = div().flex().flex_col().child(header);
        for row in rows.iter().take(6) {
            let mut line = div().flex().border_b_1().border_color(theme.border);
            for value in row {
                line = line.child(cell(value.clone(), false));
            }
            body = body.child(line);
        }
        div()
            .id("import-preview-box")
            .max_h(px(140.0))
            .overflow_scroll()
            .rounded_xs()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg_editor)
            .child(body)
    }

    fn render_destination_picker(&self, modal: &ImportModal, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let enabled = !modal.is_running;
        let selected = modal.destination.as_ref().map(|d| &d.relation);
        let active = modal.active_field == Some(ImportField::DestinationFilter);
        let mut list = div()
            .id("transfer-destinations")
            .flex()
            .flex_col()
            .max_h(px(PICKER_MAX_HEIGHT))
            .overflow_y_scroll()
            .rounded_xs()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg_editor);
        if modal.destination_matches.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_color(theme.text_muted)
                    .child("No writable tables on connected connections match"),
            );
        }
        for (i, relation) in modal.destination_matches.iter().enumerate() {
            let is_selected = selected == Some(relation);
            let hover = theme.bg_surface_hover;
            let mut row = div()
                .id(ElementId::NamedInteger("transfer-dest".into(), i as u64))
                .px_2()
                .py_0p5()
                .text_color(theme.text_primary)
                .when(is_selected, |r| r.bg(theme.bg_surface_selected))
                .child(format!(
                    "{}  {}.{}",
                    relation.conn_id, relation.schema, relation.table
                ));
            if enabled {
                let relation = relation.clone();
                row = row
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_destination(relation.clone(), cx);
                    }));
            }
            list = list.child(row);
        }
        let note = if modal.destination_matches.len() == DESTINATION_LIMIT {
            format!("Showing the first {DESTINATION_LIMIT} matches; type to narrow")
        } else {
            format!("{} match(es)", modal.destination_matches.len())
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_color(theme.text_muted).child("Destination"))
                    .child(text_box(
                        ImportField::DestinationFilter,
                        &modal.destination_filter,
                        "Filter connection, schema or table…",
                        active,
                        theme,
                        cx,
                    )),
            )
            .child(list)
            .child(div().text_color(theme.text_muted).child(note))
    }

    fn render_mapping(&self, modal: &ImportModal, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let enabled = !modal.is_running;
        let mut list = div()
            .id("import-mapping")
            .flex()
            .flex_col()
            .max_h(px(220.0))
            .overflow_y_scroll()
            .rounded_xs()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg_editor);
        for (i, row) in modal.mapping.rows.iter().enumerate() {
            let source_label = row
                .source
                .and_then(|s| modal.mapping.sources.get(s))
                .map_or("(skip)".to_string(), Clone::clone);
            let mut source_btn = pill(
                ElementId::NamedInteger("map-source".into(), i as u64),
                source_label,
                modal.picker == Some(MappingPicker::Source(i)),
                enabled,
                theme,
            );
            let mut conv_btn = pill(
                ElementId::NamedInteger("map-conv".into(), i as u64),
                conversion_label(row.conversion),
                modal.picker == Some(MappingPicker::Conversion(i)),
                enabled && row.source.is_some(),
                theme,
            );
            if enabled {
                source_btn = source_btn.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal {
                        m.picker = toggle(m.picker, MappingPicker::Source(i));
                        cx.notify();
                    }
                }));
                if row.source.is_some() {
                    conv_btn = conv_btn.on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = &mut this.import_modal {
                            m.picker = toggle(m.picker, MappingPicker::Conversion(i));
                            cx.notify();
                        }
                    }));
                }
            }
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .w(px(180.0))
                            .flex_none()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(theme.text_primary)
                            .child(format!(
                                "{} ({})",
                                row.target.name,
                                row.target.data_type.display_name()
                            )),
                    )
                    .child(div().text_color(theme.text_muted).child("←"))
                    .child(source_btn)
                    .child(conv_btn),
            );
            match modal.picker {
                Some(MappingPicker::Source(p)) if p == i => {
                    list = list.child(self.render_source_options(modal, i, cx));
                }
                Some(MappingPicker::Conversion(p)) if p == i => {
                    list = list.child(self.render_conversion_options(i, row.conversion, cx));
                }
                _ => {}
            }
        }
        list
    }

    fn render_source_options(
        &self,
        modal: &ImportModal,
        row: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let current = modal.mapping.rows[row].source;
        let options = std::iter::once((None, "(skip)".to_string())).chain(
            modal
                .mapping
                .sources
                .iter()
                .enumerate()
                .map(|(i, s)| (Some(i), s.clone())),
        );
        let mut wrap = div().flex().flex_wrap().gap_1();
        for (n, (source, label)) in options.enumerate() {
            wrap = wrap.child(
                pill(
                    ElementId::NamedInteger("map-source-opt".into(), n as u64),
                    label,
                    current == source,
                    true,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal {
                        m.mapping.set_source(row, source);
                        m.picker = None;
                        cx.notify();
                    }
                })),
            );
        }
        div()
            .id("map-source-options")
            .max_h(px(PICKER_MAX_HEIGHT))
            .overflow_y_scroll()
            .p_1()
            .bg(theme.bg_surface)
            .child(wrap)
    }

    fn render_conversion_options(
        &self,
        row: usize,
        current: db_wire::transfer::mapping::Conversion,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut wrap = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .p_1()
            .bg(self.theme.bg_surface);
        for (n, (conversion, label)) in CONVERSIONS.iter().enumerate() {
            let conversion = *conversion;
            wrap = wrap.child(
                pill(
                    ElementId::NamedInteger("map-conv-opt".into(), n as u64),
                    *label,
                    current == conversion,
                    true,
                    &self.theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal {
                        m.mapping.set_conversion(row, conversion);
                        m.picker = None;
                        cx.notify();
                    }
                })),
            );
        }
        wrap
    }

    fn render_run_options(&self, modal: &ImportModal, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let enabled = !modal.is_running;
        let mut row = div().flex().flex_wrap().items_center().gap_1();
        for (label, policy) in [
            ("Atomic Commit", CommitPolicy::Atomic),
            ("Per-Batch Commit", CommitPolicy::PerBatch),
        ] {
            let selected =
                std::mem::discriminant(&modal.commit_policy) == std::mem::discriminant(&policy);
            let mut p = pill(label, label, selected, enabled, theme);
            if enabled {
                p = p.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = &mut this.import_modal {
                        m.commit_policy = policy;
                        cx.notify();
                    }
                }));
            }
            row = row.child(p);
        }
        let mut skip = pill(
            "toggle-skip-invalid",
            "Skip invalid values",
            modal.skip_invalid,
            enabled,
            theme,
        );
        if enabled {
            skip = skip.on_click(cx.listener(|this, _, _, cx| {
                if let Some(m) = &mut this.import_modal {
                    m.skip_invalid = !m.skip_invalid;
                    cx.notify();
                }
            }));
        }
        row.child(skip)
    }

    pub(super) fn render_import_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let modal = self.import_modal.as_ref().unwrap();
        let theme = &self.theme;
        let title = match (&modal.source, &modal.destination) {
            (ImportSource::Csv(_), Some(d)) => format!(
                "Import CSV Data into {}.{} ({})",
                d.relation.schema, d.relation.table, d.relation.conn_id
            ),
            (ImportSource::Query(q), _) => format!("Transfer from {}", q.label),
            (ImportSource::Csv(_), None) => "Import CSV Data".into(),
        };
        let mut content = div().flex().flex_col().gap_3().text_size(px(11.5));
        if let Some(csv) = self.render_csv_options(modal, cx) {
            content = content.child(csv);
        }
        if !modal.destination_fixed {
            content = content.child(self.render_destination_picker(modal, cx));
        }
        if modal.destination.is_some() {
            content = content
                .child(div().text_color(theme.text_muted).child(format!(
                    "Column mapping: {} of {} destination column(s) mapped; skipped columns use their defaults",
                    modal.mapping.mapped_count(),
                    modal.mapping.rows.len()
                )))
                .child(self.render_mapping(modal, cx));
        }
        content = content.child(self.render_run_options(modal, cx));

        if let Some(err) = &modal.error {
            content = content.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_xs()
                    .border_1()
                    .border_color(theme.text_error)
                    .text_color(theme.text_error)
                    .child(err.clone()),
            );
        } else if let Some(status) = &modal.status {
            content = content.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_xs()
                    .border_1()
                    .border_color(theme.border_focused)
                    .text_color(theme.text_primary)
                    .child(status.clone()),
            );
        }

        let start_label = if modal.is_transfer() {
            "Start Transfer"
        } else {
            "Start Import"
        };
        let buttons = if modal.is_running {
            div()
                .flex()
                .justify_end()
                .child(tool("Cancel").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(control) =
                        this.import_modal.as_ref().and_then(|m| m.control.clone())
                    {
                        control.cancel();
                        cx.notify();
                    }
                })))
        } else {
            div()
                .flex()
                .justify_between()
                .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                    this.import_modal = None;
                    cx.notify();
                })))
                .child(
                    tool(start_label)
                        .bg(theme.bg_surface_selected)
                        .on_click(cx.listener(|this, _, _, cx| this.execute_import(cx))),
                )
        };
        content = content.child(buttons);

        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("import-csv-modal")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .w(px(680.0))
                    .max_h(relative(0.9))
                    .overflow_y_scroll()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border_focused)
                    .bg(theme.bg_surface)
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(14.0))
                                    .text_color(theme.text_primary)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .id("import-modal-close")
                                    .cursor_pointer()
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .child("✕")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.import_modal.as_mut().is_some_and(|m| m.dismiss()) {
                                            this.import_modal = None;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(content),
            )
    }
}

fn toggle(current: Option<MappingPicker>, next: MappingPicker) -> Option<MappingPicker> {
    (current != Some(next)).then_some(next)
}
