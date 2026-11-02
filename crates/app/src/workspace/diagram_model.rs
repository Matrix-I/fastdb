use db_wire::{ColumnMeta, ForeignKey, TableSummary};
use std::collections::{BTreeMap, HashMap};

pub(super) const WIDTH: f32 = 320.0;
pub(super) const HEADER: f32 = 32.0;
pub(super) const ROW: f32 = 24.0;

#[derive(Default)]
pub(super) struct DiagramModel {
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    pub width: f32,
    pub height: f32,
    pub notice: Option<String>,
}
pub(super) struct DiagramNode {
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnMeta>,
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub width: f32,
    pub outgoing: Vec<usize>,
}
pub(super) struct DiagramEdge {
    pub fk: ForeignKey,
    pub source: usize,
    pub target: usize,
    pub source_row: Option<usize>,
    pub target_row: Option<usize>,
}
impl DiagramModel {
    pub fn build(
        tables: &[TableSummary],
        columns: &HashMap<(String, String), Vec<ColumnMeta>>,
        keys: Vec<ForeignKey>,
        notice: Option<String>,
    ) -> Self {
        let mut entries = BTreeMap::new();
        for table in tables {
            entries.insert((table.schema.clone(), table.name.clone()), ());
        }
        for key in &keys {
            entries.insert((key.source_schema.clone(), key.source_table.clone()), ());
            entries.insert((key.target_schema.clone(), key.target_table.clone()), ());
        }
        let mut model = Self {
            notice,
            ..Default::default()
        };
        let mut indices = HashMap::new();
        for ((schema, table), _) in entries {
            indices.insert((schema.clone(), table.clone()), model.nodes.len());
            model.nodes.push(DiagramNode {
                columns: columns
                    .get(&(schema.clone(), table.clone()))
                    .cloned()
                    .unwrap_or_default(),
                schema,
                table,
                x: 0.0,
                y: 0.0,
                height: 0.0,
                width: WIDTH,
                outgoing: Vec::new(),
            });
        }
        for fk in keys {
            let source = indices[&(fk.source_schema.clone(), fk.source_table.clone())];
            let target = indices[&(fk.target_schema.clone(), fk.target_table.clone())];
            let source_row = model.nodes[source]
                .columns
                .iter()
                .position(|c| c.name == fk.source_column);
            let target_row = model.nodes[target]
                .columns
                .iter()
                .position(|c| c.name == fk.target_column);
            model.nodes[source].outgoing.push(model.edges.len());
            model.edges.push(DiagramEdge {
                fk,
                source,
                target,
                source_row,
                target_row,
            });
        }
        let mut y = 40.0;
        for band in model.nodes.chunks_mut(3) {
            let mut band_height: f32 = 0.0;
            let mut x = 40.0;
            for node in band.iter_mut() {
                node.width = node
                    .columns
                    .iter()
                    .map(|c| {
                        80.0 + c.name.chars().count() as f32 * 12.0
                            + c.raw_type
                                .as_deref()
                                .unwrap_or(c.data_type.display_name())
                                .chars()
                                .count() as f32
                                * 10.0
                    })
                    .fold(
                        WIDTH.max((node.schema.len() + node.table.len()) as f32 * 13.0 + 32.0),
                        f32::max,
                    );
                node.x = x;
                x += node.width + 100.0;
                node.y = y;
                node.height = HEADER
                    + ROW * node.columns.len().max(1) as f32
                    + 20.0 * node.outgoing.len() as f32
                    + 8.0;
                band_height = band_height.max(node.height);
                model.width = model.width.max(node.x + node.width + 60.0);
            }
            y += band_height + 100.0;
        }
        model.height = y.max(200.0);
        model
    }
    pub fn visible(&self, index: usize, x: f32, y: f32, w: f32, h: f32, zoom: f32) -> bool {
        let n = &self.nodes[index];
        n.x * zoom <= x + w
            && (n.x + n.width) * zoom >= x
            && n.y * zoom <= y + h
            && (n.y + n.height) * zoom >= y
    }
}

pub(super) struct DiagramState {
    pub scroll: gpui::ScrollHandle,
    pub zoom: f32,
    pub pan: Option<gpui::Point<gpui::Pixels>>,
    pub selected_edge: Option<usize>,
    pub selected_column: Option<(usize, usize)>,
    pub viewport: gpui::Size<gpui::Pixels>,
}
impl Default for DiagramState {
    fn default() -> Self {
        Self {
            scroll: gpui::ScrollHandle::new(),
            zoom: 1.0,
            pan: None,
            selected_edge: None,
            selected_column: None,
            viewport: gpui::size(gpui::px(0.0), gpui::px(0.0)),
        }
    }
}
impl DiagramState {
    pub fn zoom_to(&mut self, zoom: f32) {
        let bounds = self.scroll.bounds().size;
        self.zoom_at(zoom, gpui::point(bounds.width / 2.0, bounds.height / 2.0));
    }
    pub fn zoom_at(&mut self, zoom: f32, anchor: gpui::Point<gpui::Pixels>) {
        let next = zoom.clamp(0.25, 2.0);
        let offset = self.scroll.offset();
        let ratio = next / self.zoom;
        self.scroll.set_offset(gpui::point(
            (offset.x - anchor.x) * ratio + anchor.x,
            (offset.y - anchor.y) * ratio + anchor.y,
        ));
        self.zoom = next;
    }
}

pub(super) fn zoom_shortcut(key: &str) -> Option<f32> {
    match key {
        "+" | "=" | "add" => Some(1.2),
        "-" | "_" | "subtract" => Some(1.0 / 1.2),
        "0" => Some(0.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db_wire::DataType;
    #[test]
    fn qualified_graph_preserves_composite_self_and_external_links() {
        let tables = vec![
            TableSummary {
                schema: "a".into(),
                name: "same".into(),
                table_type: "TABLE".into(),
                estimated_rows: None,
            },
            TableSummary {
                schema: "b".into(),
                name: "same".into(),
                table_type: "TABLE".into(),
                estimated_rows: None,
            },
        ];
        let mut columns = HashMap::new();
        columns.insert(
            ("a".into(), "same".into()),
            vec![ColumnMeta::new("id", DataType::Int4)],
        );
        let key = |target_schema: &str, column: &str| ForeignKey {
            name: "fk".into(),
            source_schema: "a".into(),
            source_table: "same".into(),
            source_column: "id".into(),
            target_schema: target_schema.into(),
            target_table: "same".into(),
            target_column: column.into(),
        };
        let model = DiagramModel::build(
            &tables,
            &columns,
            vec![
                key("a", "id"),
                key("b", "id"),
                key("b", "part"),
                key("outside", "id"),
            ],
            None,
        );
        assert_eq!(model.nodes.len(), 3);
        assert_eq!(model.edges.len(), 4);
        assert_eq!(model.edges[0].source, model.edges[0].target);
        assert_ne!(model.edges[1].source, model.edges[1].target);
        assert_eq!(model.edges[0].source_row, Some(0));
        assert_eq!(model.edges[1].target_row, None);
        for pair in model.nodes.windows(2) {
            assert!(pair[0].x + WIDTH < pair[1].x);
        }
        assert!(model.visible(0, 0.0, 0.0, 800.0, 600.0, 1.0));
        assert!(!model.visible(0, 5000.0, 5000.0, 800.0, 600.0, 1.0));
        let again = DiagramModel::build(&tables, &columns, Vec::new(), None);
        assert_eq!(model.nodes[0].schema, again.nodes[0].schema);
    }
    #[test]
    fn tall_cards_do_not_overlap_next_band_and_last_card_is_scroll_reachable() {
        let tables: Vec<_> = (0..8)
            .map(|i| TableSummary {
                schema: "main".into(),
                name: format!("table_{i}"),
                table_type: "TABLE".into(),
                estimated_rows: None,
            })
            .collect();
        let mut columns = HashMap::new();
        columns.insert(
            ("main".into(), "table_0".into()),
            (0..100)
                .map(|i| ColumnMeta::new(format!("col_{i}"), DataType::Text))
                .collect(),
        );
        let graph = DiagramModel::build(&tables, &columns, Vec::new(), None);
        assert!(graph.nodes[0].y + graph.nodes[0].height < graph.nodes[3].y);
        assert!(graph.nodes[3].y + graph.nodes[3].height < graph.nodes[6].y);
        let last = graph.nodes.last().unwrap();
        for zoom in [0.25, 1.0, 2.0] {
            let x = (graph.width * zoom - 600.0).max(0.0).min(last.x * zoom);
            let y = (graph.height * zoom - 400.0).max(0.0).min(last.y * zoom);
            assert!(graph.visible(7, x, y, 600.0, 400.0, zoom));
            assert!(last.y + last.height <= graph.height);
        }
    }

    #[test]
    fn zoom_preserves_the_world_point_under_the_pointer() {
        let mut state = DiagramState::default();
        state
            .scroll
            .set_offset(gpui::point(gpui::px(-300.0), gpui::px(-500.0)));
        let anchor = gpui::point(gpui::px(150.0), gpui::px(200.0));
        let before = state.scroll.offset();
        state.zoom_at(1.5, anchor);
        let after = state.scroll.offset();
        assert_eq!((anchor.x - before.x) / 1.0, (anchor.x - after.x) / 1.5);
        assert_eq!((anchor.y - before.y) / 1.0, (anchor.y - after.y) / 1.5);
        for key in ["+", "=", "add"] {
            assert_eq!(zoom_shortcut(key), Some(1.2));
        }
        for key in ["-", "_", "subtract"] {
            assert_eq!(zoom_shortcut(key), Some(1.0 / 1.2));
        }
        assert_eq!(zoom_shortcut("0"), Some(0.0));
        assert_eq!(zoom_shortcut("a"), None);
    }

    #[test]
    fn zoom_is_bounded() {
        let mut state = DiagramState::default();
        state.zoom_to(100.0);
        assert_eq!(state.zoom, 2.0);
        state.zoom_to(0.0);
        assert_eq!(state.zoom, 0.25);
    }
}
