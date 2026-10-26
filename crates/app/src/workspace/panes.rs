//! Regions of the window drawn as their own cached views, so a frame that only changes
//! one region (scrolling the result grid) reuses the layout and paint of the others.
//!
//! Each pane renders a part of the [`Workspace`] and re-renders whenever the workspace
//! notifies. The grid scrolls by notifying only the table pane: its ancestors are drawn
//! again, but the navigator and top bar panes are not dirty and come from the last frame.

use super::*;
use super::top_bar::TOP_BAR_HEIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneKind {
    TopBar,
    Navigator,
    TableView,
}

pub struct Pane {
    workspace: WeakEntity<Workspace>,
    kind: PaneKind,
    _workspace_changed: Subscription,
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        self.workspace
            .update(cx, |workspace, cx| match kind {
                PaneKind::TopBar => workspace.render_top_bar(cx).into_any_element(),
                PaneKind::Navigator => workspace.render_navigator(cx).into_any_element(),
                PaneKind::TableView => workspace.render_table_view(window, cx).into_any_element(),
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

/// The cached panes of a workspace window
#[derive(Clone)]
pub struct Panes {
    top_bar: Entity<Pane>,
    navigator: Entity<Pane>,
    table_view: Entity<Pane>,
}

impl Panes {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
        let workspace = cx.entity();
        let mut pane = |kind| {
            let workspace = workspace.clone();
            cx.new(|cx| Pane {
                workspace: workspace.downgrade(),
                kind,
                _workspace_changed: cx.observe(&workspace, |_, _, cx| cx.notify()),
            })
        };
        Self {
            top_bar: pane(PaneKind::TopBar),
            navigator: pane(PaneKind::Navigator),
            table_view: pane(PaneKind::TableView),
        }
    }

    pub fn top_bar(&self) -> AnyView {
        AnyView::from(self.top_bar.clone()).cached(StyleRefinement::default().flex_none().w_full().h(px(TOP_BAR_HEIGHT)))
    }

    pub fn navigator(&self, width: f32) -> AnyView {
        AnyView::from(self.navigator.clone()).cached(StyleRefinement::default().flex_none().w(px(width)).h_full())
    }

    /// Kept from growing to the grid's full content width, which would leave the grid
    /// nothing to scroll horizontally
    pub fn table_view(&self) -> AnyView {
        AnyView::from(self.table_view.clone()).cached(StyleRefinement::default().flex_1().min_w_0().h_full().overflow_hidden())
    }
}
