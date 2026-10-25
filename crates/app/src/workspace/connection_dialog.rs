use super::*;

impl Workspace {
    /// Modal dialog for creating, editing and testing a database connection
    pub(super) fn render_connection_dialog(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let is_pg = self.selected_driver_kind == DbDriverKind::Postgres;
        let is_mysql = self.selected_driver_kind == DbDriverKind::MySql;
        let is_sqlite = self.selected_driver_kind == DbDriverKind::Sqlite;

        let is_name_active = self.active_field == Some(FormField::Name);
        let is_host_active = self.active_field == Some(FormField::Host);
        let is_port_active = self.active_field == Some(FormField::Port);
        let is_db_active = self.active_field == Some(FormField::Database);
        let is_user_active = self.active_field == Some(FormField::User);
        let is_pass_active = self.active_field == Some(FormField::Password);

        let modal_overlay = div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(540.0))
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
                                            .child(if self.editing_connection_id.is_some() {
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
                                        this.show_connection_modal = false;
                                        cx.notify();
                                    }))
                                    .child("✕")
                            )
                    )
                    // Driver Selection Tabs
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("btn-select-driver-pg")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(if is_pg { theme.bg_surface_selected } else { theme.bg_app })
                                    .border_1()
                                    .border_color(if is_pg { theme.text_accent } else { theme.border })
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_preset_postgres();
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(img("icons/postgresql.png").w(px(16.0)).h(px(16.0)))
                                            .child("PostgreSQL")
                                    )
                            )
                            .child(
                                div()
                                    .id("btn-select-driver-mysql")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(if is_mysql { theme.bg_surface_selected } else { theme.bg_app })
                                    .border_1()
                                    .border_color(if is_mysql { theme.text_accent } else { theme.border })
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_preset_mysql();
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(img("icons/mysql.png").w(px(16.0)).h(px(16.0)))
                                            .child("MySQL")
                                    )
                            )
                            .child(
                                div()
                                    .id("btn-select-driver-sqlite")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(if is_sqlite { theme.bg_surface_selected } else { theme.bg_app })
                                    .border_1()
                                    .border_color(if is_sqlite { theme.text_accent } else { theme.border })
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_preset_sqlite();
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(img("icons/sqlite.png").w(px(16.0)).h(px(16.0)))
                                            .child("SQLite")
                                    )
                            )
                    )
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
                                        div()
                                            .id("input-field-name")
                                            .p_2()
                                            .rounded_md()
                                            .bg(theme.bg_app)
                                            .border_1()
                                            .border_color(if is_name_active { theme.text_accent } else { theme.border })
                                            .text_size(px(13.0))
                                            .cursor_text()
                                            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                this.active_field = Some(FormField::Name);
                                                this.field_selected_all = event.click_count() >= 2;
                                                this.cursor_visible = true;
                                                window.focus(&this.focus_handle);
                                                cx.notify();
                                            }))
                                            .child(render_field_content(
                                                &self.form_name,
                                                "(optional - defaults to database name)",
                                                is_name_active,
                                                if is_name_active { self.field_selected_all } else { false },
                                                self.cursor_visible,
                                                theme,
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
                                                div()
                                                    .id("input-field-host")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_host_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                        this.active_field = Some(FormField::Host);
                                                        this.field_selected_all = event.click_count() >= 2;
                                                        this.cursor_visible = true;
                                                        window.focus(&this.focus_handle);
                                                        cx.notify();
                                                    }))
                                                    .child(render_field_content(
                                                        &self.form_host,
                                                        "localhost",
                                                        is_host_active,
                                                        if is_host_active { self.field_selected_all } else { false },
                                                        self.cursor_visible,
                                                        theme,
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
                                                div()
                                                    .id("input-field-port")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_port_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                        this.active_field = Some(FormField::Port);
                                                        this.field_selected_all = event.click_count() >= 2;
                                                        this.cursor_visible = true;
                                                        window.focus(&this.focus_handle);
                                                        cx.notify();
                                                    }))
                                                    .child(render_field_content(
                                                        &self.form_port,
                                                        "5432",
                                                        is_port_active,
                                                        if is_port_active { self.field_selected_all } else { false },
                                                        self.cursor_visible,
                                                        theme,
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
                                                if is_sqlite { "SQLite Database File / Path" } else { "Database Name" }
                                            ))
                                            .child(
                                                div()
                                                    .id("input-field-db")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_db_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                        this.active_field = Some(FormField::Database);
                                                        this.field_selected_all = event.click_count() >= 2;
                                                        this.cursor_visible = true;
                                                        window.focus(&this.focus_handle);
                                                        cx.notify();
                                                    }))
                                                    .child(render_field_content(
                                                        &self.form_database,
                                                        if is_sqlite { "e.g. ./test.db or :memory:" } else { "database name" },
                                                        is_db_active,
                                                        if is_db_active { self.field_selected_all } else { false },
                                                        self.cursor_visible,
                                                        theme,
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
                                                div()
                                                    .id("input-field-user")
                                                    .p_2()
                                                    .rounded_md()
                                                    .bg(theme.bg_app)
                                                    .border_1()
                                                    .border_color(if is_user_active { theme.text_accent } else { theme.border })
                                                    .text_size(px(13.0))
                                                    .cursor_text()
                                                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                        this.active_field = Some(FormField::User);
                                                        this.field_selected_all = event.click_count() >= 2;
                                                        this.cursor_visible = true;
                                                        window.focus(&this.focus_handle);
                                                        cx.notify();
                                                    }))
                                                    .child(render_field_content(
                                                        &self.form_user,
                                                        "username",
                                                        is_user_active,
                                                        if is_user_active { self.field_selected_all } else { false },
                                                        self.cursor_visible,
                                                        theme,
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
                                                        this.show_password = !this.show_password;
                                                        cx.notify();
                                                    }))
                                                    .child(if self.show_password { "🙈 Hide" } else { "👁 Show" })
                                            )
                                    )
                                    .child({
                                        let display_pass = if self.show_password {
                                            self.form_password.clone()
                                        } else {
                                            "•".repeat(self.form_password.chars().count())
                                        };
                                        div()
                                            .id("input-field-pass")
                                            .p_2()
                                            .rounded_md()
                                            .bg(theme.bg_app)
                                            .border_1()
                                            .border_color(if is_pass_active { theme.text_accent } else { theme.border })
                                            .text_size(px(13.0))
                                            .cursor_text()
                                            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                                this.active_field = Some(FormField::Password);
                                                this.field_selected_all = event.click_count() >= 2;
                                                this.cursor_visible = true;
                                                window.focus(&this.focus_handle);
                                                cx.notify();
                                            }))
                                            .child(render_field_content(
                                                &display_pass,
                                                "(optional password)",
                                                is_pass_active,
                                                if is_pass_active { self.field_selected_all } else { false },
                                                self.cursor_visible,
                                                theme,
                                            ))
                                    })
                            } else {
                                div()
                            })
                    )
                    // Test Result Notification Banner
                    .child({
                        match &self.test_result {
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
                                    .bg(if self.is_connecting { theme.bg_app } else { theme.bg_surface_hover })
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.is_connecting {
                                            this.test_connection(cx);
                                        }
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(
                                                if self.is_connecting {
                                                    div().child("⏳")
                                                } else {
                                                    div().child(img("icons/sql_validate.svg").w(px(14.0)).h(px(14.0)))
                                                }
                                            )
                                            .child(if self.is_connecting { "Testing..." } else { "Test Connection" })
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
                                                this.show_connection_modal = false;
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
                                            .bg(if self.is_connecting { theme.bg_surface_hover } else { theme.text_accent })
                                            .text_color(if self.is_connecting { theme.text_muted } else { theme.bg_app })
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(12.0))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if !this.is_connecting {
                                                    this.connect_to_database(cx);
                                                }
                                            }))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .child(
                                                        if self.is_connecting {
                                                            div().child("⏳")
                                                        } else {
                                                            div().child(img("icons/connect.svg").w(px(14.0)).h(px(14.0)))
                                                        }
                                                    )
                                                    .child(if self.is_connecting { "Connecting..." } else { "Connect Database" })
                                            )
                                    )
                            )
                    )
            );


        modal_overlay
    }
}
