use gpui::{hsla, Rgba};

#[derive(Debug, Clone)]
pub struct Theme {
    pub bg_app: Rgba,
    pub bg_sidebar: Rgba,
    pub bg_surface: Rgba,
    pub bg_surface_hover: Rgba,
    pub bg_surface_selected: Rgba,
    pub bg_editor: Rgba,
    pub bg_grid: Rgba,
    pub bg_grid_alt: Rgba,
    pub bg_grid_header: Rgba,
    pub bg_grid_cell_selected: Rgba,
    pub bg_grid_cell_modified: Rgba,

    pub border: Rgba,
    pub border_focused: Rgba,

    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,
    pub text_accent: Rgba,
    pub text_error: Rgba,
    pub text_success: Rgba,
    pub text_warning: Rgba,

    pub row_height: f32,
    pub header_height: f32,
    pub sidebar_width: f32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg_app: hsla(220.0 / 360.0, 0.13, 0.10, 1.0).into(),
            bg_sidebar: hsla(220.0 / 360.0, 0.14, 0.11, 1.0).into(),
            bg_surface: hsla(220.0 / 360.0, 0.13, 0.13, 1.0).into(),
            bg_surface_hover: hsla(220.0 / 360.0, 0.13, 0.17, 1.0).into(),
            bg_surface_selected: hsla(220.0 / 360.0, 0.25, 0.22, 1.0).into(),
            bg_editor: hsla(220.0 / 360.0, 0.13, 0.09, 1.0).into(),
            bg_grid: hsla(220.0 / 360.0, 0.13, 0.10, 1.0).into(),
            bg_grid_alt: hsla(220.0 / 360.0, 0.13, 0.12, 1.0).into(),
            bg_grid_header: hsla(220.0 / 360.0, 0.14, 0.14, 1.0).into(),
            bg_grid_cell_selected: hsla(212.0 / 360.0, 0.70, 0.25, 0.6).into(),
            bg_grid_cell_modified: hsla(38.0 / 360.0, 0.80, 0.20, 0.5).into(),

            border: hsla(220.0 / 360.0, 0.12, 0.18, 1.0).into(),
            border_focused: hsla(212.0 / 360.0, 0.85, 0.55, 1.0).into(),

            text_primary: hsla(210.0 / 360.0, 0.20, 0.95, 1.0).into(),
            text_secondary: hsla(215.0 / 360.0, 0.15, 0.70, 1.0).into(),
            text_muted: hsla(215.0 / 360.0, 0.10, 0.45, 1.0).into(),
            text_accent: hsla(212.0 / 360.0, 0.90, 0.60, 1.0).into(),
            text_error: hsla(0.0 / 360.0, 0.75, 0.60, 1.0).into(),
            text_success: hsla(142.0 / 360.0, 0.65, 0.50, 1.0).into(),
            text_warning: hsla(38.0 / 360.0, 0.85, 0.55, 1.0).into(),

            row_height: 26.0,
            header_height: 28.0,
            sidebar_width: 260.0,
        }
    }

    pub fn light() -> Self {
        Self {
            bg_app: hsla(0.0, 0.0, 0.98, 1.0).into(),
            bg_sidebar: hsla(220.0 / 360.0, 0.10, 0.95, 1.0).into(),
            bg_surface: hsla(0.0, 0.0, 1.0, 1.0).into(),
            bg_surface_hover: hsla(220.0 / 360.0, 0.10, 0.92, 1.0).into(),
            bg_surface_selected: hsla(212.0 / 360.0, 0.40, 0.88, 1.0).into(),
            bg_editor: hsla(0.0, 0.0, 1.0, 1.0).into(),
            bg_grid: hsla(0.0, 0.0, 1.0, 1.0).into(),
            bg_grid_alt: hsla(220.0 / 360.0, 0.10, 0.97, 1.0).into(),
            bg_grid_header: hsla(220.0 / 360.0, 0.10, 0.93, 1.0).into(),
            bg_grid_cell_selected: hsla(212.0 / 360.0, 0.80, 0.85, 0.7).into(),
            bg_grid_cell_modified: hsla(38.0 / 360.0, 0.90, 0.85, 0.6).into(),

            border: hsla(220.0 / 360.0, 0.10, 0.85, 1.0).into(),
            border_focused: hsla(212.0 / 360.0, 0.85, 0.45, 1.0).into(),

            text_primary: hsla(220.0 / 360.0, 0.20, 0.15, 1.0).into(),
            text_secondary: hsla(220.0 / 360.0, 0.15, 0.35, 1.0).into(),
            text_muted: hsla(220.0 / 360.0, 0.10, 0.60, 1.0).into(),
            text_accent: hsla(212.0 / 360.0, 0.90, 0.45, 1.0).into(),
            text_error: hsla(0.0 / 360.0, 0.75, 0.45, 1.0).into(),
            text_success: hsla(142.0 / 360.0, 0.65, 0.35, 1.0).into(),
            text_warning: hsla(38.0 / 360.0, 0.85, 0.40, 1.0).into(),

            row_height: 26.0,
            header_height: 28.0,
            sidebar_width: 260.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
