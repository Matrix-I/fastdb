use super::object_favorites::{self, Favorite};
use super::script_tools::tool;
use super::*;
use db_wire::objects::{ObjectKind, ObjectPage, ObjectSummary};

fn format_object_title(schema: &str, name: &str, qualifier: &str, kind: ObjectKind) -> String {
    if qualifier.is_empty() {
        format!("{schema}.{name}")
    } else {
        match kind {
            ObjectKind::Routine => format!("{schema}.{name}({qualifier})"),
            ObjectKind::Trigger => format!("{schema}.{name} ON {qualifier}"),
            ObjectKind::Type => format!("{schema}.{name} [{qualifier}]"),
            ObjectKind::Sequence => format!("{schema}.{name} ({qualifier})"),
        }
    }
}

pub(super) struct ObjectBrowser {
    owner: String,
    generation: u64,
    kind: ObjectKind,
    objects: Vec<ObjectSummary>,
    next: Option<u32>,
    message: String,
    task: Option<Task<()>>,
    favorites: Vec<Favorite>,
    selected: Option<ObjectSummary>,
}

impl Workspace {
    pub(super) fn open_object_browser(&mut self, cx: &mut Context<Self>) {
        let Some(owner) = self.connections.active_connection_id.clone() else {
            return;
        };
        self.object_browser = Some(ObjectBrowser {
            owner,
            generation: self.connections.active().metadata_generation,
            kind: ObjectKind::Sequence,
            objects: Vec::new(),
            next: None,
            message: String::new(),
            task: None,
            favorites: Vec::new(),
            selected: None,
        });
        self.browse_objects(ObjectKind::Sequence, 0, cx);
    }

    fn browse_objects(&mut self, kind: ObjectKind, after: u32, cx: &mut Context<Self>) {
        let Some(browser) = &mut self.object_browser else {
            return;
        };
        browser.task = None;
        let Some(session) = self.connections.sessions.get(&browser.owner) else {
            return;
        };
        let Some(driver) = session.driver.clone() else {
            browser.message = "Connection is disconnected. Reconnect first.".into();
            cx.notify();
            return;
        };
        browser.generation = session.metadata_generation;
        browser.kind = kind;
        browser.objects.clear();
        browser.selected = None;
        browser.next = None;
        browser.message = "Loading…".into();
        let owner = browser.owner.clone();
        let generation = browser.generation;
        let original_driver = driver.clone();
        browser.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    db_wire::get_runtime().block_on(async {
                        tokio::time::timeout(
                            std::time::Duration::from_secs(15),
                            driver.list_objects(kind, after),
                        )
                        .await
                    })
                })
                .await;
            let favorites = cx
                .background_executor()
                .spawn(async move {
                    object_favorites::read(
                        &platform_hal::paths::config_dir().join("favorites.json"),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let current = this.connections.sessions.get(&owner);
                let valid = current.is_some_and(|session| {
                    session.metadata_generation == generation
                        && session
                            .driver
                            .as_ref()
                            .is_some_and(|d| Arc::ptr_eq(d, &original_driver))
                });
                let Some(browser) = &mut this.object_browser else {
                    return;
                };
                if !valid {
                    browser.objects.clear();
                    browser.message = "Connection changed. Refresh the catalog.".into();
                    browser.task = None;
                    cx.notify();
                    return;
                }
                match favorites {
                    Ok(favorites) => browser.favorites = favorites,
                    Err(error) => {
                        browser.message = format!("Cannot read favorites: {error}");
                        browser.task = None;
                        cx.notify();
                        return;
                    }
                }
                match result {
                    Ok(Ok(Some(ObjectPage { objects, next }))) => {
                        browser.message = if objects.is_empty() {
                            "No objects found.".into()
                        } else {
                            "Select an object to read metadata/source.".into()
                        };
                        browser.objects = objects;
                        browser.next = next;
                    }
                    Ok(Ok(None)) => {
                        browser.message =
                            "Object discovery is not supported by this provider.".into()
                    }
                    Ok(Err(error)) => browser.message = error.to_string(),
                    Err(_) => browser.message = "Catalog request timed out.".into(),
                }
                browser.task = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_object_favorite(&mut self, cx: &mut Context<Self>) {
        let Some(browser) = &mut self.object_browser else {
            return;
        };
        let Some(object) = &browser.selected else {
            return;
        };
        if browser.task.is_some() {
            return;
        }
        let favorite = Favorite {
            owner: browser.owner.clone(),
            kind: format!("{:?}", browser.kind),
            schema: object.schema.clone(),
            name: object.name.clone(),
            qualifier: object.qualifier.clone(),
        };
        self.toggle_saved_favorite(favorite, cx);
    }

    fn toggle_saved_favorite(&mut self, favorite: Favorite, cx: &mut Context<Self>) {
        let Some(browser) = &mut self.object_browser else {
            return;
        };
        if browser.task.is_some() {
            return;
        }
        browser.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    object_favorites::toggle(
                        &platform_hal::paths::config_dir().join("favorites.json"),
                        favorite,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(browser) = &mut this.object_browser {
                    match result {
                        Ok(favorites) => {
                            browser.favorites = favorites;
                            browser.message = "Favorites saved.".into();
                        }
                        Err(error) => {
                            browser.message = format!("Could not save favorites: {error}")
                        }
                    }
                    browser.task = None;
                    cx.notify();
                }
            });
        }));
    }

    fn open_favorite(&mut self, favorite: Favorite, cx: &mut Context<Self>) {
        let Some(browser) = &mut self.object_browser else {
            return;
        };
        let Some(kind) = favorite.object_kind() else {
            browser.message = format!("Unknown object kind in favorite: {}", favorite.kind);
            cx.notify();
            return;
        };
        let Some(session) = self.connections.sessions.get(&browser.owner) else {
            return;
        };
        let Some(driver) = session.driver.clone() else {
            browser.message = "Connection is disconnected. Reconnect first.".into();
            cx.notify();
            return;
        };
        browser.task = None;
        browser.kind = kind;
        browser.message = "Locating favorite…".into();
        browser.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    db_wire::get_runtime().block_on(async {
                        tokio::time::timeout(std::time::Duration::from_secs(15), async {
                            let mut after = 0;
                            for _ in 0..50 {
                                let page = driver
                                    .list_objects(kind, after)
                                    .await?
                                    .ok_or_else(|| anyhow::anyhow!("Provider unsupported"))?;
                                if let Some(object) = page.objects.into_iter().find(|o| {
                                    o.schema == favorite.schema
                                        && o.name == favorite.name
                                        && o.qualifier == favorite.qualifier
                                }) {
                                    return anyhow::Ok(object);
                                }
                                match page.next {
                                    Some(next) => after = next,
                                    None => break,
                                }
                            }
                            anyhow::bail!("Favorite no longer exists; refresh the catalog")
                        })
                        .await
                        .map_err(|_| anyhow::anyhow!("Favorite lookup timed out"))?
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(browser) = &mut this.object_browser {
                    browser.task = None;
                    match result {
                        Ok(object) => this.browse_object_source(kind, object, cx),
                        Err(error) => {
                            browser.message = error.to_string();
                            cx.notify();
                        }
                    }
                }
            });
        }));
    }

    fn browse_object_source(
        &mut self,
        kind: ObjectKind,
        object: ObjectSummary,
        cx: &mut Context<Self>,
    ) {
        let Some(browser) = &mut self.object_browser else {
            return;
        };
        browser.task = None;
        let Some(session) = self.connections.sessions.get(&browser.owner) else {
            return;
        };
        if session.metadata_generation != browser.generation || session.driver.is_none() {
            browser.message = "Connection metadata changed. Refresh this list.".into();
            cx.notify();
            return;
        }
        let driver = session.driver.clone().unwrap();
        browser.kind = kind;
        browser.selected = Some(object.clone());
        browser.message = "Loading source…".into();
        let owner = browser.owner.clone();
        let generation = browser.generation;
        let original_driver = driver.clone();
        browser.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    db_wire::get_runtime().block_on(async {
                        tokio::time::timeout(
                            std::time::Duration::from_secs(15),
                            driver.object_source(kind, &object),
                        )
                        .await
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let current = this.connections.sessions.get(&owner);
                let valid = current.is_some_and(|session| {
                    session.metadata_generation == generation
                        && session
                            .driver
                            .as_ref()
                            .is_some_and(|d| Arc::ptr_eq(d, &original_driver))
                });
                let Some(browser) = &mut this.object_browser else {
                    return;
                };
                if !valid {
                    browser.message = "Connection changed. Refresh the catalog.".into();
                    browser.task = None;
                    cx.notify();
                    return;
                }
                browser.message = match result {
                    Ok(Ok(Some(source))) => source,
                    Ok(Ok(None)) => "Source is unsupported.".into(),
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => "Source request timed out.".into(),
                };
                browser.task = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render_object_browser(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let browser = self.object_browser.as_ref().unwrap();
        let mut categories = div().flex().gap_2();
        for (label, kind) in [
            ("Sequences", ObjectKind::Sequence),
            ("Functions/procedures", ObjectKind::Routine),
            ("Triggers", ObjectKind::Trigger),
            ("Types", ObjectKind::Type),
        ] {
            let active = kind == browser.kind;
            let mut button = tool(label);
            if active {
                button = button.bg(self.theme.bg_surface_hover);
            }
            categories = categories.child(
                button
                    .on_click(cx.listener(move |this, _, _, cx| this.browse_objects(kind, 0, cx))),
            );
        }
        let mut rows = div()
            .id("catalog-object-list")
            .max_h(px(240.))
            .overflow_y_scroll();
        for (i, object) in browser.objects.iter().enumerate() {
            let object = object.clone();
            let is_selected = browser
                .selected
                .as_ref()
                .is_some_and(|sel| sel.id == object.id);
            let title = format_object_title(
                &object.schema,
                &object.name,
                &object.qualifier,
                browser.kind,
            );
            let mut row = div().id(("catalog-object", i)).p_2().cursor_pointer();
            if is_selected {
                row = row.bg(self.theme.bg_surface_hover);
            }
            let row = row.child(title).on_click(cx.listener({
                let object = object.clone();
                let kind = browser.kind;
                move |this, _, _, cx| this.browse_object_source(kind, object.clone(), cx)
            }));
            rows = rows.child(row);
        }
        let kind = browser.kind;
        let mut actions = div()
            .flex()
            .gap_2()
            .child(
                tool("Refresh")
                    .on_click(cx.listener(move |this, _, _, cx| this.browse_objects(kind, 0, cx))),
            )
            .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                this.object_browser = None;
                cx.notify();
            })));
        if browser.selected.is_some() {
            actions = actions.child(
                tool("Toggle favorite")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_object_favorite(cx))),
            );
        }
        let mut favorites = div()
            .id("catalog-favorites")
            .max_h(px(100.))
            .overflow_y_scroll()
            .child("Favorites");
        for (i, favorite) in browser
            .favorites
            .iter()
            .filter(|f| f.owner == browser.owner && f.kind == format!("{:?}", browser.kind))
            .enumerate()
        {
            let favorite = favorite.clone();
            let remove = favorite.clone();
            let fav_title = format_object_title(
                &favorite.schema,
                &favorite.name,
                &favorite.qualifier,
                browser.kind,
            );
            favorites = favorites.child(
                div()
                    .id(("favorite", i))
                    .cursor_pointer()
                    .child(fav_title)
                    .child(
                        tool("Remove favorite").on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_saved_favorite(remove.clone(), cx);
                        })),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.open_favorite(favorite.clone(), cx)),
                    ),
            );
        }
        if let Some(after) = browser.next {
            actions =
                actions.child(tool("Next page").on_click(
                    cx.listener(move |this, _, _, cx| this.browse_objects(kind, after, cx)),
                ));
        }
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
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .w(px(720.))
                    .rounded_md()
                    .border_1()
                    .border_color(self.theme.border_focused)
                    .bg(self.theme.bg_surface)
                    .child("Catalog objects")
                    .child(categories)
                    .child(favorites)
                    .child(rows)
                    .child(
                        div()
                            .id("catalog-source")
                            .max_h(px(220.))
                            .overflow_y_scroll()
                            .child(browser.message.clone()),
                    )
                    .child(actions),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::format_object_title;
    use db_wire::objects::ObjectKind;

    #[test]
    fn format_object_titles_by_kind_and_qualifier() {
        assert_eq!(
            format_object_title("public", "users_seq", "", ObjectKind::Sequence),
            "public.users_seq"
        );
        assert_eq!(
            format_object_title("public", "add", "a integer, b integer", ObjectKind::Routine),
            "public.add(a integer, b integer)"
        );
        assert_eq!(
            format_object_title("public", "on_change", "orders", ObjectKind::Trigger),
            "public.on_change ON orders"
        );
        assert_eq!(
            format_object_title("public", "status", "enum", ObjectKind::Type),
            "public.status [enum]"
        );
    }
}
