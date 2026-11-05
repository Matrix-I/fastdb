use super::connection_state::ConnectionSettingsTab;
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
                        if !this
                            .form
                            .visible_settings_fields()
                            .contains(&this.form.active_field.unwrap_or(FormField::Name))
                        {
                            this.form.active_field = None;
                            this.form.field_anchor = None;
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

    fn render_read_only_setting(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.form.read_only && !self.form.selected_driver_kind.supports_read_only() {
            return div().into_any_element();
        }
        div()
            .id("toggle-read-only")
            .cursor_pointer()
            .text_size(px(12.0))
            .on_click(cx.listener(|this, _, _, cx| {
                this.form.read_only = !this.form.read_only;
                this.form.test_result = None;
                this.form.invalidate();
                cx.notify();
            }))
            .child(if self.form.read_only {
                "☑ Read-only"
            } else {
                "☐ Read-only"
            })
            .into_any_element()
    }

    fn render_connection_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let mut fields = div().flex().gap_2();
        for (field, label, value, placeholder) in [
            (
                FormField::ConnectionGroup,
                "Navigator group",
                self.form.connection_group.as_str(),
                "Optional, e.g. Development",
            ),
            (
                FormField::ConnectionColor,
                "Navigator color",
                self.form.connection_color.as_str(),
                "Optional, e.g. #d29922",
            ),
        ] {
            let active = self.form.active_field == Some(field);
            let pick = self.field_caret_pick(field, cx);
            fields = fields.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(label),
                    )
                    .child(
                        ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(theme.bg_app)
                            .border_1()
                            .border_color(if active {
                                theme.border_focused
                            } else {
                                theme.border
                            })
                            .text_size(px(12.0))
                            .cursor_text()
                            .overflow_hidden()
                            .child(render_field_content(
                                value,
                                placeholder,
                                active,
                                self.form.field_anchor,
                                self.form.field_cursor,
                                self.input.cursor_visible,
                                theme,
                                &pick,
                            )),
                    ),
            );
        }
        fields.into_any_element()
    }

    fn render_client_identity(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.form.shows_client_identity() {
            return div().into_any_element();
        }
        let masked = "•".repeat(self.form.client_archive_password.chars().count());
        let theme = &self.theme;
        let mut fields = div().flex().gap_2();
        for (field, label, value, placeholder) in [
            (
                FormField::ClientArchive,
                "PKCS#12 client identity",
                self.form.client_archive.as_str(),
                "Optional .p12 or .pfx path",
            ),
            (
                FormField::ClientArchivePassword,
                "Archive password",
                masked.as_str(),
                "Optional",
            ),
        ] {
            let active = self.form.active_field == Some(field);
            let pick = self.field_caret_pick(field, cx);
            fields = fields.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(label),
                    )
                    .child(
                        ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(theme.bg_app)
                            .border_1()
                            .border_color(if active {
                                theme.border_focused
                            } else {
                                theme.border
                            })
                            .text_size(px(12.0))
                            .cursor_text()
                            .overflow_hidden()
                            .child(render_field_content(
                                value,
                                placeholder,
                                active,
                                self.form.field_anchor,
                                self.form.field_cursor,
                                self.input.cursor_visible,
                                theme,
                                &pick,
                            )),
                    ),
            );
        }
        fields.into_any_element()
    }

    fn render_connection_timeout(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.form.selected_driver_kind == DbDriverKind::Sqlite {
            return div().into_any_element();
        }
        let theme = &self.theme;
        let field = FormField::ConnectTimeout;
        let active = self.form.active_field == Some(field);
        let pick = self.field_caret_pick(field, cx);
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.0))
            .child("Connection timeout (seconds)")
            .child(
                ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                    .w(px(64.0))
                    .px_2()
                    .py_1()
                    .cursor_text()
                    .rounded_md()
                    .border_1()
                    .border_color(if active {
                        theme.border_focused
                    } else {
                        theme.border
                    })
                    .bg(theme.bg_app)
                    .child(render_field_content(
                        &self.form.connect_timeout,
                        "10",
                        active,
                        self.form.field_anchor,
                        self.form.field_cursor,
                        self.input.cursor_visible,
                        theme,
                        &pick,
                    )),
            )
            .into_any_element()
    }

    fn render_application_name(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.form.selected_driver_kind != DbDriverKind::Postgres
            && self.form.application_name.is_empty()
        {
            return div().into_any_element();
        }
        let theme = &self.theme;
        let field = FormField::ApplicationName;
        let active = self.form.active_field == Some(field);
        let pick = self.field_caret_pick(field, cx);
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.0))
            .child("PostgreSQL application name")
            .child(
                ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                    .w(px(240.0))
                    .px_2()
                    .py_1()
                    .cursor_text()
                    .rounded_md()
                    .border_1()
                    .border_color(if active {
                        theme.border_focused
                    } else {
                        theme.border
                    })
                    .bg(theme.bg_app)
                    .child(render_field_content(
                        &self.form.application_name,
                        "Optional",
                        active,
                        self.form.field_anchor,
                        self.form.field_cursor,
                        self.input.cursor_visible,
                        theme,
                        &pick,
                    )),
            )
            .into_any_element()
    }

    fn render_ssh_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.form.selected_driver_kind.supports_ssh() && !self.form.ssh.enabled {
            return div().into_any_element();
        }
        let ssh = &self.form.ssh;
        let mut section = div().flex().flex_col().gap_2().child(
            div()
                .id("toggle-ssh")
                .cursor_pointer()
                .text_size(px(12.0))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.form.invalidate();
                    this.form.test_result = None;
                    this.form.ssh.enabled = !this.form.ssh.enabled;
                    this.form.active_field = this.form.ssh.enabled.then_some(FormField::SshHost);
                    this.form.field_cursor = usize::MAX;
                    this.form.field_anchor = None;
                    cx.notify();
                }))
                .child(if ssh.enabled {
                    "☑ Connect through SSH"
                } else {
                    "☐ Connect through SSH"
                }),
        );
        if ssh.enabled {
            let masked = "•".repeat(ssh.secret.chars().count());
            for row in [
                [
                    (
                        FormField::SshHost,
                        "SSH host",
                        ssh.host.as_str(),
                        "jump.example.com",
                    ),
                    (FormField::SshPort, "SSH port", ssh.port.as_str(), "22"),
                ],
                [
                    (
                        FormField::SshUser,
                        "SSH username",
                        ssh.user.as_str(),
                        "operator",
                    ),
                    (
                        FormField::SshFingerprint,
                        "Trusted host key fingerprint",
                        ssh.fingerprint.as_str(),
                        "SHA256:…",
                    ),
                ],
                [
                    (
                        FormField::SshKey,
                        "Private key path",
                        ssh.key_path.as_str(),
                        "Optional for password authentication",
                    ),
                    (
                        FormField::SshSecret,
                        "SSH password or key passphrase",
                        masked.as_str(),
                        "Optional for an unencrypted key",
                    ),
                ],
            ] {
                let mut fields = div().flex().gap_2();
                for (field, label, value, placeholder) in row {
                    let theme = &self.theme;
                    let active = self.form.active_field == Some(field);
                    let pick = self.field_caret_pick(field, cx);
                    fields = fields.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_secondary)
                                    .child(label),
                            )
                            .child(
                                ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(theme.bg_app)
                                    .border_1()
                                    .border_color(if active {
                                        theme.border_focused
                                    } else {
                                        theme.border
                                    })
                                    .text_size(px(12.0))
                                    .cursor_text()
                                    .overflow_hidden()
                                    .child(render_field_content(
                                        value,
                                        placeholder,
                                        active,
                                        self.form.field_anchor,
                                        self.form.field_cursor,
                                        self.input.cursor_visible,
                                        theme,
                                        &pick,
                                    )),
                            ),
                    );
                }
                section = section.child(fields);
            }
        }
        section.into_any_element()
    }

    fn render_provider_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let mut providers = div().flex().flex_wrap().gap_3();
        for (label, icon, kind) in [
            ("PostgreSQL", "icons/postgresql.svg", DbDriverKind::Postgres),
            ("Db2 for LUW", "icons/database.svg", DbDriverKind::Db2),
            ("MariaDB", "icons/database.svg", DbDriverKind::MariaDb),
            ("MySQL", "icons/mysql.svg", DbDriverKind::MySql),
            ("Oracle", "icons/database.svg", DbDriverKind::Oracle),
            ("SQL Server", "icons/database.svg", DbDriverKind::SqlServer),
            ("SQLite", "icons/sqlite.svg", DbDriverKind::Sqlite),
        ] {
            let available = ConnectionFormState::provider_is_available(kind);
            let selected = kind == self.form.selected_driver_kind;
            let tile = div()
                .id(label)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .w(relative(0.23))
                .max_w(px(180.0))
                .h(px(128.0))
                .px_3()
                .py_3()
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
                .text_color(if available {
                    theme.text_primary
                } else {
                    theme.text_muted
                })
                .child(img(icon).w(px(44.0)).h(px(44.0)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_0p5()
                        .text_size(px(12.0))
                        .child(label)
                        .when(!available, |label| {
                            label.child(
                                div()
                                    .text_size(px(10.0))
                                    .child(ConnectionFormState::FEATURE_COMING_MESSAGE),
                            )
                        }),
                );
            let tile = tile.when(available, |tile| {
                tile.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.form.choose_provider(kind);
                        cx.notify();
                    }))
            });
            providers = providers.child(tile);
        }
        providers.into_any_element()
    }

    fn render_main_connection_field(
        &self,
        field: (FormField, &str, &str, &str),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (field, label, value, placeholder) = field;
        let theme = &self.theme;
        let active = self.form.active_field == Some(field);
        let pick = self.field_caret_pick(field, cx);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(label.to_string())
                    .when(field == FormField::Password, |label| {
                        label.child(
                            div()
                                .id("btn-toggle-show-pass")
                                .text_color(theme.text_accent)
                                .cursor_pointer()
                                .child(if self.form.show_password {
                                    "Hide"
                                } else {
                                    "Show"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.form.show_password = !this.form.show_password;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .bg(theme.bg_app)
                    .border_1()
                    .border_color(if active {
                        theme.border_focused
                    } else {
                        theme.border
                    })
                    .text_size(px(13.0))
                    .cursor_text()
                    .overflow_hidden()
                    .child(render_field_content(
                        value,
                        placeholder,
                        active,
                        self.form.field_anchor,
                        self.form.field_cursor,
                        self.input.cursor_visible,
                        theme,
                        &pick,
                    )),
            )
            .into_any_element()
    }

    fn render_main_connection_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let sqlite = self.form.selected_driver_kind == DbDriverKind::Sqlite;
        let password = if self.form.show_password {
            self.form.form_password.clone()
        } else {
            "•".repeat(self.form.form_password.chars().count())
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_main_connection_field(
                (
                    FormField::Name,
                    "Connection name",
                    &self.form.form_name,
                    "Optional — defaults to database name",
                ),
                cx,
            ))
            .when(!sqlite, |form| {
                form.child(
                    div()
                        .flex()
                        .gap_3()
                        .child(self.render_main_connection_field(
                            (FormField::Host, "Host", &self.form.form_host, "localhost"),
                            cx,
                        ))
                        .child(div().w(px(120.0)).child(self.render_main_connection_field(
                            (FormField::Port, "Port", &self.form.form_port, "5432"),
                            cx,
                        ))),
                )
            })
            .child(self.render_main_connection_field(
                (
                    FormField::Database,
                    if sqlite {
                        "SQLite database file / path"
                    } else if self.form.selected_driver_kind == DbDriverKind::Oracle {
                        "Service name"
                    } else {
                        "Database"
                    },
                    &self.form.form_database,
                    if sqlite {
                        "./test.db or :memory:"
                    } else {
                        "Database name"
                    },
                ),
                cx,
            ))
            .when(!sqlite, |form| {
                form.child(
                    div()
                        .flex()
                        .gap_3()
                        .child(self.render_main_connection_field(
                            (
                                FormField::User,
                                "Username",
                                &self.form.form_user,
                                "Username",
                            ),
                            cx,
                        ))
                        .child(self.render_main_connection_field(
                            (FormField::Password, "Password", &password, "Optional"),
                            cx,
                        )),
                )
            })
            .into_any_element()
    }

    fn render_connection_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let provider_available =
            ConnectionFormState::provider_is_available(self.form.selected_driver_kind);
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .text_size(px(12.0))
            .px_5()
            .py_3()
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
                    .bg(if self.form.is_connecting || !provider_available {
                        theme.bg_app
                    } else {
                        theme.bg_surface_hover
                    })
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(12.0))
                    .when(provider_available, |button| button.cursor_pointer())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.form.is_connecting
                            && ConnectionFormState::provider_is_available(
                                this.form.selected_driver_kind,
                            )
                        {
                            this.test_connection(cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(if self.form.is_connecting {
                                div().child("⏳")
                            } else {
                                div().child(img("icons/sql_validate.svg").w(px(14.0)).h(px(14.0)))
                            })
                            .child(if self.form.is_connecting {
                                "Testing..."
                            } else {
                                "Test Connection"
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("back-connection-step")
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.border)
                            .cursor_pointer()
                            .child("← Back")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.form.show_provider_selection();
                                cx.notify();
                            })),
                    )
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
                                this.form.invalidate();
                                this.form.show_connection_modal = false;
                                cx.notify();
                            }))
                            .child("Cancel"),
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
                            .child("Save"),
                    )
                    .child(
                        div()
                            .id("btn-connect-database")
                            .flex()
                            .items_center()
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .bg(if self.form.is_connecting || !provider_available {
                                theme.bg_surface_hover
                            } else {
                                theme.text_accent
                            })
                            .text_color(if self.form.is_connecting || !provider_available {
                                theme.text_muted
                            } else {
                                theme.bg_app
                            })
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.0))
                            .when(provider_available, |button| button.cursor_pointer())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.form.is_connecting
                                    && ConnectionFormState::provider_is_available(
                                        this.form.selected_driver_kind,
                                    )
                                {
                                    this.connect_to_database(cx);
                                }
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(if self.form.is_connecting {
                                        div().child("⏳")
                                    } else {
                                        div()
                                            .child(img("icons/connect.svg").w(px(14.0)).h(px(14.0)))
                                    })
                                    .child(if self.form.is_connecting {
                                        "Connecting..."
                                    } else {
                                        "Connect Database"
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_connection_settings_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let mut tabs = div()
            .flex()
            .flex_shrink_0()
            .gap_1()
            .border_b_1()
            .border_color(theme.border);
        for tab in self.form.settings_tabs() {
            let label = match tab {
                ConnectionSettingsTab::Main => "Main",
                ConnectionSettingsTab::Ssh => "SSH",
                ConnectionSettingsTab::Tls => "TLS",
                ConnectionSettingsTab::Advanced => "Advanced",
            };
            let selected = self.form.settings_tab == tab;
            tabs = tabs.child(
                div()
                    .id(label)
                    .px_4()
                    .py_2()
                    .text_size(px(12.0))
                    .border_b_2()
                    .border_color(if selected {
                        theme.text_accent
                    } else {
                        theme.border
                    })
                    .text_color(if selected {
                        theme.text_accent
                    } else {
                        theme.text_secondary
                    })
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.form.select_settings_tab(tab);
                        cx.notify();
                    })),
            );
        }
        tabs.into_any_element()
    }

    fn render_connection_settings_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.form.settings_tab {
            ConnectionSettingsTab::Main => self.render_main_connection_settings(cx),
            ConnectionSettingsTab::Ssh => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(self.render_ssh_settings(cx))
                .into_any_element(),
            ConnectionSettingsTab::Tls => div()
                .flex()
                .flex_col()
                .gap_4()
                .when(self.form.selected_driver_kind == DbDriverKind::Sqlite, |section| {
                    section.child(div().text_size(px(12.0)).text_color(self.theme.text_secondary)
                        .child("SQLite does not use TLS. Clear the retained client identity to continue."))
                })
                .child(self.render_tls_settings(cx))
                .child(self.render_client_identity(cx))
                .into_any_element(),
            ConnectionSettingsTab::Advanced => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(self.render_connection_appearance(cx))
                .child(self.render_read_only_setting(cx))
                .child(self.render_connection_timeout(cx))
                .child(self.render_application_name(cx))
                .into_any_element(),
        }
    }

    fn render_connection_feedback(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let mut feedback = div()
            .flex_shrink_0()
            .px_5()
            .py_2()
            .h(px(40.0))
            .flex()
            .items_center()
            .gap_3()
            .text_size(px(12.0));
        if let Some(result) = &self.form.test_result {
            let (message, color) = match result {
                Ok(message) => (message, theme.text_success),
                Err(message) => (message, theme.text_error),
            };
            let copy = message.clone();
            feedback = feedback
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(color)
                        .child(message.clone()),
                )
                .child(
                    div()
                        .id("copy-connection-feedback")
                        .flex_shrink_0()
                        .cursor_pointer()
                        .text_color(theme.text_accent)
                        .child("Copy details")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                        })),
                );
        }
        feedback.into_any_element()
    }

    /// Both steps share a frame so navigation never moves the footer or resizes the dialog.
    pub(super) fn render_connection_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let choosing = self.form.choosing_provider;
        div().absolute().inset_0().bg(rgba(0x00000088))
            .flex().items_center().justify_center()
            .child(div().id("connection-dialog").w(px(840.0)).h(px(600.0))
                .max_w(relative(0.96)).max_h(relative(0.94))
                .flex().flex_col().overflow_hidden()
                .bg(theme.bg_surface).border_1().border_color(theme.border_focused)
                .rounded_lg().shadow_lg()
                .child(div().flex().flex_shrink_0().items_center().justify_between().px_5().py_4()
                    .border_b_1().border_color(theme.border)
                    .child(div().flex().flex_col().gap_1()
                        .child(div().text_size(px(17.0)).font_weight(FontWeight::BOLD)
                            .child(if self.form.editing_connection_id.is_some() { "Edit Database Connection" } else { "Connect to a database" }))
                        .child(div().text_size(px(12.0)).text_color(theme.text_secondary)
                            .child(if choosing { "Step 1 of 2 · Select your database".to_string() }
                                else { format!("Step 2 of 2 · {} connection settings", self.form.selected_driver_kind.name()) })))
                    .child(div().id("btn-close-modal-x").cursor_pointer().text_color(theme.text_muted).child("✕")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.form.invalidate();
                            this.form.show_connection_modal = false;
                            cx.notify();
                        }))))
                .child(div().flex_1().min_h_0().flex().flex_col().px_5().py_3().gap_3()
                    .when(choosing, |body| body
                        .child(div().text_size(px(12.0)).text_color(theme.text_secondary)
                            .child("Choose a database type to configure your connection."))
                        .child(self.render_provider_picker(cx)))
                    .when(!choosing, |body| body
                        .child(self.render_connection_settings_tabs(cx))
                        .when(!ConnectionFormState::provider_is_available(self.form.selected_driver_kind), |body| body
                            .child(div().text_size(px(12.0)).text_color(theme.text_muted)
                                .child(ConnectionFormState::FEATURE_COMING_MESSAGE)))
                        .when(self.form.selected_driver_kind.uses_odbc(), |body| body
                            .child(div().text_size(px(11.0)).text_color(theme.text_secondary)
                                .child(if self.form.selected_driver_kind == DbDriverKind::Oracle {
                                    "Requires Oracle ODBC client. Database is the Oracle service name."
                                } else { "Requires the vendor ODBC driver installed on this computer." })))
                        .child(self.render_connection_settings_content(cx))))
                .when(!choosing, |panel| panel
                    .child(self.render_connection_feedback(cx))
                    .child(self.render_connection_actions(cx)))
                .when(choosing, |panel| panel
                    .child(div().flex_shrink_0().flex().justify_end().gap_2().px_5().py_3()
                        .border_t_1().border_color(theme.border).text_size(px(12.0))
                        .child(div().id("cancel-provider-selection").px_3().py_1p5().rounded_md()
                            .border_1().border_color(theme.border).cursor_pointer().child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.form.invalidate();
                                this.form.show_connection_modal = false;
                                cx.notify();
                            })))
                        .child(div().id("next-connection-step").px_4().py_1p5().rounded_md()
                            .bg(theme.text_accent).text_color(theme.bg_app).cursor_pointer().child("Next →")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.form.show_configuration();
                                cx.notify();
                            }))))))
    }
}
