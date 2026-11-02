use super::*;
use gpui::prelude::FluentBuilder;

impl Workspace {
    fn render_tls_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.form.selected_driver_kind == DbDriverKind::Sqlite {
            return div().into_any_element();
        }
        let theme = &self.theme;
        let accepts_ca = !self.form.selected_driver_kind.uses_odbc()
            || self.form.selected_driver_kind == DbDriverKind::Db2;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("toggle-tls")
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.form.form_tls_enabled = !this.form.form_tls_enabled;
                        this.form.test_result = None;
                        if !this.form.form_tls_enabled
                            && this.form.active_field == Some(FormField::TlsCa)
                        {
                            this.form.active_field = Some(FormField::Password);
                        }
                        cx.notify();
                    }))
                    .child(if self.form.form_tls_enabled {
                        "☑ TLS — verify certificate and hostname (TCP)"
                    } else {
                        "☐ TLS — verify certificate and hostname (TCP)"
                    }),
            )
            .when(self.form.form_tls_enabled && !accepts_ca, |tls| {
                tls.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child("Uses the system/vendor certificate trust store."),
                )
            })
            .when(self.form.form_tls_enabled && accepts_ca, |tls| {
                tls.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_secondary)
                                .child("Root CA certificate (optional; system trust when empty)"),
                        )
                        .child(
                            ui_editor::line_input::pick_surface(
                                div(),
                                usize::MAX,
                                &self.field_caret_pick(FormField::TlsCa, cx),
                            )
                            .id("tls-ca-path")
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(theme.bg_app)
                            .border_1()
                            .border_color(if self.form.active_field == Some(FormField::TlsCa) {
                                theme.border_focused
                            } else {
                                theme.border
                            })
                            .text_size(px(12.0))
                            .cursor_text()
                            .child(render_field_content(
                                &self.form.form_tls_ca_cert,
                                "/path/to/ca.pem or ca.der",
                                self.form.active_field == Some(FormField::TlsCa),
                                self.form.field_anchor,
                                self.form.field_cursor,
                                self.input.cursor_visible,
                                theme,
                                &self.field_caret_pick(FormField::TlsCa, cx),
                            )),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_provider_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let mut providers = div().flex().flex_wrap().gap_2();
        for (label, icon, kind) in [
            ("PostgreSQL", "icons/postgresql.png", DbDriverKind::Postgres),
            ("Db2 for LUW", "icons/database.svg", DbDriverKind::Db2),
            ("MariaDB", "icons/database.svg", DbDriverKind::MariaDb),
            ("MySQL", "icons/mysql.png", DbDriverKind::MySql),
            ("Oracle", "icons/database.svg", DbDriverKind::Oracle),
            ("SQL Server", "icons/database.svg", DbDriverKind::SqlServer),
            ("SQLite", "icons/sqlite.png", DbDriverKind::Sqlite),
        ] {
            let selected = kind == self.form.selected_driver_kind;
            let tile = div()
                .id(label)
                .flex()
                .items_center()
                .gap_2()
                .w(px(150.0))
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .bg(if selected {
                    theme.bg_surface_selected
                } else {
                    theme.bg_app
                })
                .border_color(if selected {
                    theme.text_accent
                } else {
                    theme.border
                })
                .text_color(theme.text_primary)
                .child(img(icon).w(px(24.0)).h(px(24.0)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .text_size(px(12.0))
                        .child(label),
                );
            let tile = tile
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.form.select_preset(kind);
                    cx.notify();
                }));
            providers = providers.child(tile);
        }
        providers.into_any_element()
    }

    /// Modal dialog for creating, editing and testing a database connection
    pub(super) fn render_connection_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let is_sqlite = self.form.selected_driver_kind == DbDriverKind::Sqlite;

        let is_name_active = self.form.active_field == Some(FormField::Name);
        let is_host_active = self.form.active_field == Some(FormField::Host);
        let is_port_active = self.form.active_field == Some(FormField::Port);
        let is_db_active = self.form.active_field == Some(FormField::Database);
        let is_user_active = self.form.active_field == Some(FormField::User);
        let is_pass_active = self.form.active_field == Some(FormField::Password);

        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(680.0))
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(theme.border_focused)
                    .rounded_lg()
                    .shadow_lg()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    // Modal Header
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(theme.border)
                            .pb_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_size(px(16.0))
                                            .font_weight(FontWeight::BOLD)
                                            .child(img("icons/connect.svg").w(px(18.0)).h(px(18.0)))
                                            .child(if self.form.editing_connection_id.is_some() {
                                                "Edit Database Connection"
                                            } else {
                                                "New Database Connection"
                                            })
                                    )
                            )
                            .child(
                                div()
                                    .id("btn-close-modal-x")
                                    .text_size(px(14.0))
                                    .text_color(theme.text_muted)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.form.invalidate(); this.form.show_connection_modal = false;
                                        cx.notify();
                                    }))
                                    .child("✕")
                            )
                    )
                    .child(self.render_provider_picker(cx))
                    .when(self.form.selected_driver_kind.uses_odbc(), |panel| panel.child(
                        div().text_size(px(11.0)).text_color(theme.text_secondary)
                            .child(if self.form.selected_driver_kind == DbDriverKind::Oracle {
                                "Requires Oracle ODBC client. Database is the Oracle service name."
                            } else { "Requires the vendor ODBC driver installed on this computer." })
                    ))
                    // Form Input Fields
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            // Connection Name (Optional)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Connection Name (Optional)"))
                                    .child(
                                        ui_editor::line_input::pick_surface(div(), usize::MAX, &self.field_caret_pick(FormField::Name, cx))
                                            .id("input-field-name")
                                            .p_2()
                                            .rounded_md()
                                            .bg(theme.bg_app)
                                            .border_1()
                                            .border_color(if is_name_active { theme.text_accent } else { theme.border })
                                            .text_size(px(13.0))
                                            .cursor_text()
                                            .child(render_field_content(
                                                &self.form.form_name,
                                                "(optional - defaults to database name)",
                                                is_name_active,
                                                self.form.field_anchor,
                                                self.form.field_cursor,
                                                self.input.cursor_visible,
                                                theme,
                                                &self.field_caret_pick(FormField::Name, cx),
                                            ))
                                    )
                            )
                            // Host & Port row (hidden for SQLite)
                            .child(if !is_sqlite {
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .gap_1()
                                            .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Host"))
                                            .child(
                                                ui_editor::line_input::pick_surface(div(), usize::MAX, &self.field_caret_pick(FormField::Host, cx))
                                                    .id("input-field-host")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_host_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .child(render_field_content(
                                                        &self.form.form_host,
                                                        "localhost",
                                                        is_host_active,
                                                        self.form.field_anchor,
                                                        self.form.field_cursor,
                                                        self.input.cursor_visible,
                                                        theme,
                                                        &self.field_caret_pick(FormField::Host, cx),
                                                    ))
                                            )
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .w(px(120.0))
                                            .gap_1()
                                            .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Port"))
                                            .child(
                                                ui_editor::line_input::pick_surface(div(), usize::MAX, &self.field_caret_pick(FormField::Port, cx))
                                                    .id("input-field-port")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_port_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .child(render_field_content(
                                                        &self.form.form_port,
                                                        "5432",
                                                        is_port_active,
                                                        self.form.field_anchor,
                                                        self.form.field_cursor,
                                                        self.input.cursor_visible,
                                                        theme,
                                                        &self.field_caret_pick(FormField::Port, cx),
                                                    ))
                                            )
                                    )
                            } else {
                                div()
                            })
                            // Database Name / SQLite File row
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .gap_1()
                                            .child(div().text_size(px(11.0)).text_color(theme.text_muted).child(
                                                if is_sqlite { "SQLite Database File / Path" } else if self.form.selected_driver_kind == DbDriverKind::Oracle { "Service Name" } else { "Database Name" }
                                            ))
                                            .child(
                                                ui_editor::line_input::pick_surface(div(), usize::MAX, &self.field_caret_pick(FormField::Database, cx))
                                                    .id("input-field-db")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_db_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .child(render_field_content(
                                                        &self.form.form_database,
                                                        if is_sqlite { "e.g. ./test.db or :memory:" } else { "database name" },
                                                        is_db_active,
                                                        self.form.field_anchor,
                                                        self.form.field_cursor,
                                                        self.input.cursor_visible,
                                                        theme,
                                                        &self.field_caret_pick(FormField::Database, cx),
                                                    ))
                                            )
                                    )
                                    .child(if !is_sqlite {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .gap_1()
                                            .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Username"))
                                            .child(
                                                ui_editor::line_input::pick_surface(div(), usize::MAX, &self.field_caret_pick(FormField::User, cx))
                                                    .id("input-field-user")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_user_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .child(render_field_content(
                                                        &self.form.form_user,
                                                        "username",
                                                        is_user_active,
                                                        self.form.field_anchor,
                                                        self.form.field_cursor,
                                                        self.input.cursor_visible,
                                                        theme,
                                                        &self.field_caret_pick(FormField::User, cx),
                                                    ))
                                            )
                                    } else {
                                        div()
                                    })
                            )
                            // Password row (hidden for SQLite)
                            .child(if !is_sqlite {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().text_size(px(11.0)).text_color(theme.text_muted).child("Password"))
                                            .child(
                                                div()
                                                    .id("btn-toggle-show-pass")
                                                    .text_size(px(11.0))
                                                    .text_color(theme.text_accent)
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.form.show_password = !this.form.show_password;
                                                        cx.notify();
                                                    }))
                                                    .child(if self.form.show_password { "🙈 Hide" } else { "👁 Show" })
                                            )
                                    )
                                    .child({
                                        let display_pass = if self.form.show_password {
                                            self.form.form_password.clone()
                                        } else {
                                            "•".repeat(self.form.form_password.chars().count())
                                        };
                                        ui_editor::line_input::pick_surface(div(), usize::MAX, &self.field_caret_pick(FormField::Password, cx))
                                            .id("input-field-pass")
                                            .p_2()
                                            .rounded_md()
                                            .bg(theme.bg_app)
                                            .border_1()
                                            .border_color(if is_pass_active { theme.text_accent } else { theme.border })
                                            .text_size(px(13.0))
                                            .cursor_text()
                                            .child(render_field_content(
                                                &display_pass,
                                                "(optional password)",
                                                is_pass_active,
                                                self.form.field_anchor,
                                                self.form.field_cursor,
                                                self.input.cursor_visible,
                                                theme,
                                                &self.field_caret_pick(FormField::Password, cx),
                                            ))
                                    })
                            } else {
                                div()
                            })
                    )
                    .child(self.render_tls_settings(cx))
                    // Test Result Notification Banner
                    .child({
                        match &self.form.test_result {
                            Some(Ok(msg)) => div()
                                .p_2()
                                .rounded_md()
                                .bg(theme.bg_surface_selected)
                                .text_color(theme.text_success)
                                .text_size(px(12.0))
                                .child(format!("✓ {}", msg)),
                            Some(Err(err)) => div()
                                .p_2()
                                .rounded_md()
                                .bg(theme.bg_surface_hover)
                                .text_color(theme.text_error)
                                .text_size(px(12.0))
                                .child(format!("✕ {}", err)),
                            None => div(),
                        }
                    })
                    // Modal Action Buttons
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt_2()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .id("btn-test-connection")
                                    .flex()
                                    .items_center()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(if self.form.is_connecting { theme.bg_app } else { theme.bg_surface_hover })
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.form.is_connecting {
                                            this.test_connection(cx);
                                        }
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(
                                                if self.form.is_connecting {
                                                    div().child("⏳")
                                                } else {
                                                    div().child(img("icons/sql_validate.svg").w(px(14.0)).h(px(14.0)))
                                                }
                                            )
                                            .child(if self.form.is_connecting { "Testing..." } else { "Test Connection" })
                                    )
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("btn-cancel-modal")
                                            .flex()
                                            .items_center()
                                            .px_3()
                                            .py_1p5()
                                            .rounded_md()
                                            .bg(theme.bg_surface_hover)
                                            .border_1()
                                            .border_color(theme.border)
                                            .text_size(px(12.0))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.form.invalidate(); this.form.show_connection_modal = false;
                                                cx.notify();
                                            }))
                                            .child("Cancel")
                                    )
                                    .child(
                                        div()
                                            .id("btn-save-conn-only")
                                            .flex()
                                            .items_center()
                                            .px_3()
                                            .py_1p5()
                                            .rounded_md()
                                            .bg(theme.bg_surface_hover)
                                            .border_1()
                                            .border_color(theme.border)
                                            .text_size(px(12.0))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.save_connection_form();
                                                cx.notify();
                                            }))
                                            .child("Save")
                                    )
                                    .child(
                                        div()
                                            .id("btn-connect-database")
                                            .flex()
                                            .items_center()
                                            .px_4()
                                            .py_1p5()
                                            .rounded_md()
                                            .bg(if self.form.is_connecting { theme.bg_surface_hover } else { theme.text_accent })
                                            .text_color(if self.form.is_connecting { theme.text_muted } else { theme.bg_app })
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(12.0))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if !this.form.is_connecting {
                                                    this.connect_to_database(cx);
                                                }
                                            }))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .child(
                                                        if self.form.is_connecting {
                                                            div().child("⏳")
                                                        } else {
                                                            div().child(img("icons/connect.svg").w(px(14.0)).h(px(14.0)))
                                                        }
                                                    )
                                                    .child(if self.form.is_connecting { "Connecting..." } else { "Connect Database" })
                                            )
                                    )
                            )
                    )
            )
    }
}
