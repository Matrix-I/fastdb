use db_wire::{ColumnMeta, ForeignKey, TableSummary};
use std::collections::{BTreeMap, HashMap};

pub(super) const WIDTH: f32 = 180.0;
pub(super) const HEADER: f32 = 32.0;
pub(super) const ROW: f32 = 24.0;
pub(super) const HEADER_FONT: f32 = 12.0;
pub(super) const NAME_FONT: f32 = 11.0;
pub(super) const CARD_PADDING: f32 = 8.0;

pub(super) fn single_line_label(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\r', '\n', '\t']) {
        text.replace("\r\n", "⏎")
            .replace(['\r', '\n'], "⏎")
            .replace('\t', "⇥")
            .into()
    } else {
        text.into()
    }
}

#[derive(Clone, Default)]
pub(super) struct DiagramModel {
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    pub width: f32,
    pub height: f32,
    pub notice: Option<String>,
    root: Option<usize>,
    routes: Vec<Vec<(f32, f32)>>,
}
#[derive(Clone)]
pub(super) struct DiagramNode {
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnMeta>,
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub width: f32,
    pub outgoing: Vec<usize>,
    pub incoming: Vec<usize>,
    side: Option<usize>,
}
#[derive(Clone)]
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
                incoming: Vec::new(),
                side: None,
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
            model.nodes[target].incoming.push(model.edges.len());
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
                    .map(|c| c.name.chars().count() as f32 * 8.0 + CARD_PADDING * 2.0 + 2.0)
                    .fold(
                        WIDTH.max(
                            (node.schema.len() + node.table.len() + 1) as f32 * 8.0
                                + CARD_PADDING * 2.0
                                + 2.0,
                        ),
                        f32::max,
                    );
                node.x = x;
                x += node.width + 100.0;
                node.y = y;
                node.height = HEADER + ROW * node.columns.len().max(1) as f32 + 8.0;
                band_height = band_height.max(node.height);
                model.width = model.width.max(node.x + node.width + 60.0);
            }
            y += band_height + 100.0;
        }
        model.height = y.max(200.0);
        model.cache_routes();
        model
    }
    pub fn focused(&self, schema: &str, table: &str) -> Self {
        let is_root = |node: &DiagramNode| node.schema == schema && node.table == table;
        let keys: Vec<_> = self
            .edges
            .iter()
            .filter(|edge| is_root(&self.nodes[edge.source]) || is_root(&self.nodes[edge.target]))
            .map(|edge| edge.fk.clone())
            .collect();
        let mut included = std::collections::HashSet::from([(schema.to_owned(), table.to_owned())]);
        for key in &keys {
            included.insert((key.source_schema.clone(), key.source_table.clone()));
            included.insert((key.target_schema.clone(), key.target_table.clone()));
        }
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| included.contains(&(node.schema.clone(), node.table.clone())))
            .collect();
        let tables: Vec<_> = nodes
            .iter()
            .map(|node| TableSummary {
                schema: node.schema.clone(),
                name: node.table.clone(),
                table_type: "TABLE".into(),
                estimated_rows: None,
            })
            .collect();
        let columns = nodes
            .iter()
            .map(|node| {
                (
                    (node.schema.clone(), node.table.clone()),
                    node.columns.clone(),
                )
            })
            .collect();
        let mut graph = Self::build(&tables, &columns, keys, self.notice.clone());
        let Some(root) = graph.nodes.iter().position(is_root) else {
            return graph;
        };
        graph.layout_focused(root);
        graph.cache_routes();
        graph
    }

    fn layout_focused(&mut self, root: usize) {
        // Balance neighbors around all four sides instead of growing one tall list.
        let graph = self;
        graph.root = Some(root);
        let mut sides: [Vec<usize>; 4] = Default::default();
        for index in 0..graph.nodes.len() {
            if index == root {
                continue;
            }
            let outgoing = graph.nodes[root]
                .outgoing
                .iter()
                .any(|&edge| graph.edges[edge].target == index);
            let preferred = if outgoing { 1 } else { 0 };
            let side = [preferred, 2, 3, 1 - preferred]
                .into_iter()
                .min_by_key(|&side| sides[side].len())
                .unwrap();
            sides[side].push(index);
        }
        let gap = 100.0;
        let span = |side: usize, horizontal: bool| {
            sides[side]
                .iter()
                .map(|&i| {
                    if horizontal {
                        graph.nodes[i].width
                    } else {
                        graph.nodes[i].height
                    }
                })
                .sum::<f32>()
                + 48.0 * sides[side].len().saturating_sub(1) as f32
        };
        let breadth = |side: usize, horizontal: bool| {
            sides[side]
                .iter()
                .map(|&i| {
                    if horizontal {
                        graph.nodes[i].height
                    } else {
                        graph.nodes[i].width
                    }
                })
                .fold(0.0, f32::max)
        };
        let center_width = graph.nodes[root]
            .width
            .max(span(2, true))
            .max(span(3, true));
        let center_height = graph.nodes[root]
            .height
            .max(span(0, false))
            .max(span(1, false));
        let widths = [breadth(0, false), breadth(1, false)];
        let heights = [breadth(2, true), breadth(3, true)];
        let left = 40.0
            + if sides[0].is_empty() {
                0.0
            } else {
                widths[0] + gap
            };
        let top = 40.0
            + if sides[2].is_empty() {
                0.0
            } else {
                heights[0] + gap
            };
        let spans = [span(0, false), span(1, false), span(2, true), span(3, true)];
        graph.nodes[root].x = left + (center_width - graph.nodes[root].width) / 2.0;
        graph.nodes[root].y = top + (center_height - graph.nodes[root].height) / 2.0;
        for (side, indices) in sides.iter().enumerate() {
            let horizontal = side >= 2;
            let mut offset = if horizontal {
                left + (center_width - spans[side]) / 2.0
            } else {
                top + (center_height - spans[side]) / 2.0
            };
            for &index in indices {
                let node = &mut graph.nodes[index];
                node.side = Some(side);
                if horizontal {
                    node.x = offset;
                    node.y = if side == 2 {
                        top - gap - node.height
                    } else {
                        top + center_height + gap
                    };
                    offset += node.width + 48.0;
                } else {
                    node.x = if side == 0 {
                        left - gap - node.width
                    } else {
                        left + center_width + gap
                    };
                    node.y = offset;
                    offset += node.height + 48.0;
                }
            }
        }
        graph.width = graph
            .nodes
            .iter()
            .map(|n| n.x + n.width + 80.0)
            .fold(200.0, f32::max);
        graph.height = graph
            .nodes
            .iter()
            .map(|n| n.y + n.height + 80.0)
            .fold(200.0, f32::max);
    }

    fn fit_card_widths(&mut self, mut measure: impl FnMut(&str, f32, bool) -> f32) {
        let Some(root) = self.root else {
            return;
        };
        for node in &mut self.nodes {
            let header = format!("{}.{}", node.schema, node.table);
            let mut width = measure(&single_line_label(&header), HEADER_FONT, true);
            if node.columns.is_empty() {
                width = width.max(measure("Columns unavailable", 10.0, false));
            }
            for column in &node.columns {
                width = width.max(measure(&single_line_label(&column.name), NAME_FONT, false));
            }
            node.width = width.ceil() + CARD_PADDING * 2.0 + 2.0;
        }
        self.layout_focused(root);
        self.cache_routes();
    }

    fn restore_positions(&mut self, positions: Vec<super::diagram_positions::Position>) {
        if positions.is_empty() {
            return;
        }
        let positions: HashMap<_, _> = positions
            .into_iter()
            .filter(|p| p.x.is_finite() && p.y.is_finite() && p.x >= 0.0 && p.y >= 0.0)
            .map(|p| ((p.schema, p.table), (p.x, p.y)))
            .collect();
        for node in &mut self.nodes {
            if let Some(&(x, y)) = positions.get(&(node.schema.clone(), node.table.clone())) {
                node.x = x;
                node.y = y;
            }
        }
        self.update_geometry();
    }

    fn update_geometry(&mut self) {
        if let Some(root) = self.root {
            let top = self.nodes[root].y;
            let bottom = top + self.nodes[root].height;
            for (index, node) in self.nodes.iter_mut().enumerate() {
                node.side = if index == root {
                    None
                } else if node.y + node.height + 80.0 <= top {
                    Some(2)
                } else if node.y >= bottom + 80.0 {
                    Some(3)
                } else {
                    None
                };
            }
        }
        self.width = self
            .nodes
            .iter()
            .map(|n| n.x + n.width + 80.0)
            .fold(200.0, f32::max);
        self.height = self
            .nodes
            .iter()
            .map(|n| n.y + n.height + 80.0)
            .fold(200.0, f32::max);
        self.cache_routes();
    }

    pub fn mapped_column(&self, selected: Option<usize>, node: usize, column: usize) -> bool {
        let Some(selected) = selected.and_then(|index| self.edges.get(index)) else {
            return false;
        };
        self.edges.iter().any(|edge| {
            edge.source == selected.source
                && edge.target == selected.target
                && edge.fk.name == selected.fk.name
                && ((edge.source == node && edge.source_row == Some(column))
                    || (edge.target == node && edge.target_row == Some(column)))
        })
    }

    pub fn route(&self, index: usize) -> &[(f32, f32)] {
        &self.routes[index]
    }

    fn cache_routes(&mut self) {
        let mut counts = HashMap::new();
        for edge in &self.edges {
            let source = (edge.source, edge.source_row);
            let target = (edge.target, edge.target_row);
            *counts.entry(source).or_insert(0_usize) += 1;
            if source != target {
                *counts.entry(target).or_insert(0) += 1;
            }
        }
        let mut ranks = HashMap::new();
        let mut port = |(node, row): (usize, Option<usize>)| {
            let rank = ranks.entry((node, row)).or_insert(0_usize);
            *rank += 1;
            let center =
                self.nodes[node].y + row.map_or(HEADER / 2.0, |r| HEADER + ROW * (r as f32 + 0.5));
            // Keep separate ports inside the same column row, even for repeated references.
            center + ROW * 0.75 * (*rank as f32 / (counts[&(node, row)] + 1) as f32 - 0.5)
        };
        let mut horizontal = std::collections::HashSet::new();
        let mut vertical = std::collections::HashSet::new();
        self.routes = self
            .edges
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                let source = (edge.source, edge.source_row);
                let target = (edge.target, edge.target_row);
                let source_port = port(source);
                let (source_port, target_port) = if source == target {
                    let center = self.nodes[edge.source].y
                        + edge
                            .source_row
                            .map_or(HEADER / 2.0, |r| HEADER + ROW * (r as f32 + 0.5));
                    (center - ROW / 6.0, center + ROW / 6.0)
                } else {
                    (source_port, port(target))
                };
                let source_y = reserve_coordinate(source_port, 2.0, &mut horizontal);
                let target_y = reserve_coordinate(target_port, 2.0, &mut horizontal);
                self.compute_route(index, source_y, target_y, &mut horizontal, &mut vertical)
            })
            .collect();
    }

    fn compute_route(
        &self,
        index: usize,
        source_y: f32,
        target_y: f32,
        horizontal: &mut std::collections::HashSet<u32>,
        vertical: &mut std::collections::HashSet<u32>,
    ) -> Vec<(f32, f32)> {
        let edge = &self.edges[index];
        let source = &self.nodes[edge.source];
        let target = &self.nodes[edge.target];
        let lane_fraction = (index + 1) as f32 / (self.edges.len() + 1) as f32;
        let vertical_neighbor = if source.side.is_some_and(|side| side >= 2) {
            Some((source, target, source_y, target_y, true))
        } else if target.side.is_some_and(|side| side >= 2) {
            Some((target, source, target_y, source_y, false))
        } else {
            None
        };
        if let Some((neighbor, root, neighbor_y, root_y, reverse)) = vertical_neighbor {
            let right = neighbor.x + neighbor.width / 2.0 >= root.x + root.width / 2.0;
            let root_x = root.x + if right { root.width } else { 0.0 };
            let neighbor_x = neighbor.x + if right { 0.0 } else { neighbor.width };
            let root_offset = 16.0 + lane_fraction * 56.0;
            let root_lane = root_x + if right { root_offset } else { -root_offset };
            let neighbor_offset = 8.0 + lane_fraction * 12.0;
            let neighbor_lane = neighbor_x
                + if right {
                    -neighbor_offset
                } else {
                    neighbor_offset
                };
            let root_lane = reserve_coordinate(root_lane, 4.0, vertical);
            let neighbor_lane = reserve_coordinate(neighbor_lane, 2.0, vertical);
            let gap_y = if neighbor.side == Some(2) {
                root.y - 24.0 - lane_fraction * 48.0
            } else {
                root.y + root.height + 24.0 + lane_fraction * 48.0
            };
            let gap_y = reserve_coordinate(gap_y, 8.0, horizontal);
            let mut route = vec![
                (root_x, root_y),
                (root_lane, root_y),
                (root_lane, gap_y),
                (neighbor_lane, gap_y),
                (neighbor_lane, neighbor_y),
                (neighbor_x, neighbor_y),
            ];
            if reverse {
                route.reverse();
            }
            return route;
        }
        let y1 = source_y;
        let y2 = target_y;
        if edge.source != edge.target
            && source.x < target.x + target.width + 20.0
            && target.x < source.x + source.width + 20.0
        {
            let x1 = source.x + source.width;
            let x2 = target.x + target.width;
            let lane = reserve_coordinate(x1.max(x2) + 24.0 + lane_fraction * 32.0, 4.0, vertical);
            return vec![(x1, y1), (lane, y1), (lane, y2), (x2, y2)];
        }
        let right = target.x > source.x;
        let x1 = source.x
            + if right || edge.source == edge.target {
                source.width
            } else {
                0.0
            };
        let x2 = target.x + if right { 0.0 } else { target.width };
        let lane = if edge.source == edge.target {
            x1 + 24.0 + lane_fraction * 32.0
        } else {
            x1 + (x2 - x1) * (0.2 + lane_fraction * 0.6)
        };
        let lane = reserve_coordinate(lane, 4.0, vertical);
        vec![(x1, y1), (lane, y1), (lane, y2), (x2, y2)]
    }

    pub fn visible(&self, index: usize, x: f32, y: f32, w: f32, h: f32, zoom: f32) -> bool {
        let n = &self.nodes[index];
        n.x * zoom <= x + w
            && (n.x + n.width) * zoom >= x
            && n.y * zoom <= y + h
            && (n.y + n.height) * zoom >= y
    }
}

// Reserve coordinates across all links, since an endpoint row can coincide with
// another link's bend even when their nominal lanes differ.
fn reserve_coordinate(
    preferred: f32,
    radius: f32,
    used: &mut std::collections::HashSet<u32>,
) -> f32 {
    if used.insert(preferred.to_bits()) {
        return preferred;
    }
    let slots = used.len() + 1;
    for slot in 0..slots {
        let candidate = preferred - radius + 2.0 * radius * (slot + 1) as f32 / (slots + 1) as f32;
        if used.insert(candidate.to_bits()) {
            return candidate;
        }
    }
    // Extremely dense/large coordinates can round neighboring candidates together.
    // At most `used.len()` representable values can be occupied.
    let mut candidate = preferred;
    for _ in 0..=used.len() {
        candidate = candidate.next_up();
        if used.insert(candidate.to_bits()) {
            return candidate;
        }
    }
    unreachable!("finite diagram coordinates have an available adjacent value")
}

struct FocusedDiagram {
    base: std::sync::Arc<DiagramModel>,
    schema: String,
    table: String,
    graph: std::sync::Arc<DiagramModel>,
}

pub(super) struct DiagramState {
    pub scroll: gpui::ScrollHandle,
    pub zoom: f32,
    pub pan: Option<gpui::Point<gpui::Pixels>>,
    pub selected_edge: Option<usize>,
    pub selected_column: Option<(usize, usize)>,
    pub viewport: gpui::Size<gpui::Pixels>,
    center_pending: bool,
    pub drag: Option<(usize, gpui::Point<gpui::Pixels>, f32, f32)>,
    pub layout_error: Option<String>,
    layout_path: Option<std::path::PathBuf>,
    focused: Option<FocusedDiagram>,
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
            focused: None,
            center_pending: false,
            drag: None,
            layout_error: None,
            layout_path: None,
        }
    }
}
impl DiagramState {
    pub fn graph(
        &mut self,
        graph: std::sync::Arc<DiagramModel>,
        schema: &str,
        table: &str,
        measure: impl FnMut(&str, f32, bool) -> f32,
    ) -> std::sync::Arc<DiagramModel> {
        if let Some(cached) = &self.focused
            && std::sync::Arc::ptr_eq(&cached.base, &graph)
            && cached.schema == schema
            && cached.table == table
        {
            return cached.graph.clone();
        }
        let mut focused = graph.focused(schema, table);
        focused.fit_card_widths(measure);
        if let Some(path) = &self.layout_path {
            match super::diagram_positions::load(path) {
                Ok(positions) => focused.restore_positions(positions),
                Err(error) => {
                    self.layout_error = Some(format!("Could not load diagram positions: {error}"))
                }
            }
        }
        let focused = std::sync::Arc::new(focused);
        self.focused = Some(FocusedDiagram {
            base: graph,
            schema: schema.to_owned(),
            table: table.to_owned(),
            graph: focused.clone(),
        });
        self.selected_edge = None;
        self.selected_column = None;
        self.center_pending = true;
        self.drag = None;
        focused
    }

    pub fn set_layout_path(&mut self, path: Option<std::path::PathBuf>) {
        if self.layout_path != path {
            self.layout_path = path;
            self.focused = None;
            self.drag = None;
            self.layout_error = None;
        }
    }

    pub fn move_card(&mut self, pointer: gpui::Point<gpui::Pixels>) {
        if let Some((node, start, x, y)) = self.drag
            && let Some(focused) = &mut self.focused
        {
            let delta = pointer - start;
            let graph = std::sync::Arc::make_mut(&mut focused.graph);
            if let Some(card) = graph.nodes.get_mut(node) {
                card.x = (x + f32::from(delta.x) / self.zoom).max(0.0);
                card.y = (y + f32::from(delta.y) / self.zoom).max(0.0);
                graph.update_geometry();
            }
        }
    }

    pub fn finish_drag(&mut self, pointer: gpui::Point<gpui::Pixels>) {
        if let Some((index, start, _, _)) = self.drag.take() {
            if start == pointer {
                return;
            }
            if let (Some(path), Some(focused)) = (&self.layout_path, &self.focused) {
                let mut positions = match super::diagram_positions::load(path) {
                    Ok(positions) => positions,
                    Err(error) => {
                        self.layout_error =
                            Some(format!("Could not update diagram positions: {error}"));
                        return;
                    }
                };
                let Some(node) = focused.graph.nodes.get(index) else {
                    return;
                };
                positions.retain(|p| p.schema != node.schema || p.table != node.table);
                positions.push(super::diagram_positions::Position {
                    schema: node.schema.clone(),
                    table: node.table.clone(),
                    x: node.x,
                    y: node.y,
                });
                self.layout_error = super::diagram_positions::save(path, &positions)
                    .err()
                    .map(|error| format!("Could not save diagram positions: {error}"));
            }
        }
    }

    pub fn center_initial_view(
        &mut self,
        graph: &DiagramModel,
        size: gpui::Size<gpui::Pixels>,
    ) -> bool {
        if !self.center_pending || size.width <= gpui::px(0.0) || size.height <= gpui::px(0.0) {
            return false;
        }
        self.center_pending = false;
        if let Some(root) = graph.root.map(|i| &graph.nodes[i]) {
            let x = ((root.x + root.width / 2.0) * self.zoom - f32::from(size.width) / 2.0).clamp(
                0.0,
                (graph.width * self.zoom - f32::from(size.width)).max(0.0),
            );
            let y = ((root.y + root.height / 2.0) * self.zoom - f32::from(size.height) / 2.0)
                .clamp(
                    0.0,
                    (graph.height * self.zoom - f32::from(size.height)).max(0.0),
                );
            self.scroll
                .set_offset(gpui::point(gpui::px(-x), gpui::px(-y)));
        }
        true
    }

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
    fn multiline_labels_use_visible_single_line_markers() {
        assert_eq!(
            single_line_label("first\r\nsecond\nthird\tpart"),
            "first⏎second⏎third⇥part"
        );
        assert_eq!(single_line_label("ordinary"), "ordinary");
    }

    #[test]
    fn measured_card_fits_widest_column_name_without_types() {
        let mut wide_type = ColumnMeta::new("id", DataType::Text);
        wide_type.raw_type = Some("timestamp with time zone".into());
        let mut wide_name =
            ColumnMeta::new("termsAndConditionsAcceptanceTimestamp", DataType::Int4);
        wide_name.raw_type = Some("int".into());
        let tables = [TableSummary {
            schema: "s".into(),
            name: "t".into(),
            table_type: "TABLE".into(),
            estimated_rows: None,
        }];
        let columns = HashMap::from([(("s".into(), "t".into()), vec![wide_type, wide_name])]);
        let mut graph = DiagramModel::build(&tables, &columns, vec![], None).focused("s", "t");
        graph.fit_card_widths(|value, _, _| value.len() as f32 * 5.0);
        // "termsAndConditionsAcceptanceTimestamp" has 37 chars * 5.0 = 185.0.
        // Padding is CARD_PADDING * 2.0 (16.0) + 2.0 = 18.0. Total = 203.0.
        // Neither "int" (15.0) nor "timestamp with time zone" (120.0) are added to column width.
        assert_eq!(graph.nodes[0].width, 203.0);
        assert!(graph.width >= graph.nodes[0].x + graph.nodes[0].width);

        // Verify that a card with only short column names does not expand for long raw types.
        let only_type_col = HashMap::from([(
            ("s".into(), "t".into()),
            vec![ColumnMeta {
                raw_type: Some(
                    "very long data type definition that should not affect width".into(),
                ),
                ..ColumnMeta::new("id", DataType::Text)
            }],
        )]);
        let mut type_graph =
            DiagramModel::build(&tables, &only_type_col, vec![], None).focused("s", "t");
        type_graph.fit_card_widths(|value, _, _| value.len() as f32 * 5.0);
        // Header "s.t" is 3 * 5 = 15.0, col "id" is 2 * 5 = 10.0. Max = 15.0.
        // 15.0 + 18.0 = 33.0.
        assert_eq!(type_graph.nodes[0].width, 33.0);
    }

    #[test]
    fn dragging_at_zoom_saves_world_position_for_a_new_tab() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.json");
        let tables = [TableSummary {
            schema: "s".into(),
            name: "root".into(),
            table_type: "TABLE".into(),
            estimated_rows: None,
        }];
        let base = std::sync::Arc::new(DiagramModel::build(&tables, &HashMap::new(), vec![], None));
        let mut tab = DiagramState::default();
        tab.set_layout_path(Some(path.clone()));
        let graph = tab.graph(base.clone(), "s", "root", |value, size, _| {
            value.len() as f32 * size
        });
        let card = &graph.nodes[0];
        let point = |x, y| gpui::point(gpui::px(x), gpui::px(y));
        tab.zoom = 2.0;
        tab.drag = Some((0, point(100.0, 100.0), card.x, card.y));
        let drop = point(140.0, 160.0);
        tab.move_card(drop);
        assert!(!path.exists());
        tab.finish_drag(drop);
        assert!(tab.layout_error.is_none());
        let mut reopened = DiagramState::default();
        reopened.set_layout_path(Some(path));
        let reopened = reopened.graph(base, "s", "root", |value, size, _| {
            value.len() as f32 * size
        });
        assert_eq!(
            (reopened.nodes[0].x, reopened.nodes[0].y),
            (card.x + 20.0, card.y + 30.0)
        );
    }

    #[test]
    fn repeated_same_column_self_links_keep_both_ports_in_the_column_row() {
        let columns = HashMap::from([(
            ("s".into(), "root".into()),
            vec![
                ColumnMeta::new("id", DataType::Int4),
                ColumnMeta::new("other", DataType::Int4),
            ],
        )]);
        let keys = (0..32)
            .map(|i| ForeignKey {
                name: format!("self_{i}"),
                source_schema: "s".into(),
                source_table: "root".into(),
                source_column: "id".into(),
                target_schema: "s".into(),
                target_table: "root".into(),
                target_column: "id".into(),
            })
            .collect();
        let graph = DiagramModel::build(&[], &columns, keys, None).focused("s", "root");
        let top = graph.nodes[0].y + HEADER;
        for i in 0..graph.edges.len() {
            let route = graph.route(i);
            for port in [route[0], *route.last().unwrap()] {
                assert!(
                    port.1 > top && port.1 < top + ROW,
                    "self link {i} leaves its column row"
                );
            }
        }
    }

    #[test]
    fn selecting_composite_constraint_highlights_only_its_mapped_columns() {
        let columns = HashMap::from([
            (
                ("a".into(), "child".into()),
                vec![
                    ColumnMeta::new("x", DataType::Int4),
                    ColumnMeta::new("y", DataType::Int4),
                    ColumnMeta::new("z", DataType::Int4),
                ],
            ),
            (
                ("b".into(), "parent".into()),
                vec![
                    ColumnMeta::new("id", DataType::Int4),
                    ColumnMeta::new("part", DataType::Int4),
                    ColumnMeta::new("other", DataType::Int4),
                ],
            ),
        ]);
        let key = |name: &str, from: &str, to: &str| ForeignKey {
            name: name.into(),
            source_schema: "a".into(),
            source_table: "child".into(),
            source_column: from.into(),
            target_schema: "b".into(),
            target_table: "parent".into(),
            target_column: to.into(),
        };
        let graph = DiagramModel::build(
            &[],
            &columns,
            vec![
                key("composite", "x", "id"),
                key("composite", "y", "part"),
                key("separate", "z", "other"),
            ],
            None,
        )
        .focused("a", "child");
        let edge = &graph.edges[0];
        for row in [0, 1] {
            assert!(graph.mapped_column(Some(0), edge.source, row));
            assert!(graph.mapped_column(Some(0), edge.target, row));
        }
        assert!(!graph.mapped_column(Some(0), edge.source, 2));
        assert!(!graph.mapped_column(Some(0), edge.target, 2));
        assert!(!graph.mapped_column(Some(2), edge.source, 0));
        assert!(graph.mapped_column(Some(2), edge.source, 2));
        assert!(graph.focused("missing", "table").nodes.is_empty());
    }

    #[test]
    fn neighbors_surround_root_without_card_or_endpoint_intersections() {
        let tables: Vec<_> = (0..18)
            .map(|i| TableSummary {
                schema: "s".into(),
                name: format!("t{i:02}"),
                table_type: "TABLE".into(),
                estimated_rows: None,
            })
            .collect();
        let columns = tables
            .iter()
            .map(|t| {
                (
                    (t.schema.clone(), t.name.clone()),
                    (0..if t.name == "t00" { 20 } else { 4 })
                        .map(|i| ColumnMeta::new(format!("c{i}"), DataType::Int4))
                        .collect(),
                )
            })
            .collect();
        let keys = (1..18)
            .flat_map(|i| {
                (0..2).map(move |part| ForeignKey {
                    name: format!("fk{i}_{part}"),
                    source_schema: "s".into(),
                    source_table: if i % 2 == 0 {
                        format!("t{i:02}")
                    } else {
                        "t00".into()
                    },
                    source_column: "c1".into(),
                    target_schema: "s".into(),
                    target_table: if i % 2 == 0 {
                        "t00".into()
                    } else {
                        format!("t{i:02}")
                    },
                    target_column: "c0".into(),
                })
            })
            .collect();
        let base = std::sync::Arc::new(DiagramModel::build(&tables, &columns, keys, None));
        let mut state = DiagramState::default();
        let graph = state.graph(base.clone(), "s", "t00", |value, size, _| {
            value.len() as f32 * size
        });
        for side in 0..4 {
            assert!((4..=5).contains(&graph.nodes.iter().filter(|n| n.side == Some(side)).count()));
        }
        for (i, a) in graph.nodes.iter().enumerate() {
            assert!(a.x >= 0.0 && a.y >= 0.0);
            assert!(a.x + a.width <= graph.width && a.y + a.height <= graph.height);
            for b in &graph.nodes[i + 1..] {
                assert!(
                    a.x + a.width < b.x
                        || b.x + b.width < a.x
                        || a.y + a.height < b.y
                        || b.y + b.height < a.y
                );
            }
        }
        for (i, edge) in graph.edges.iter().enumerate() {
            let route = graph.route(i);
            assert!(
                (route[0].1 - (graph.nodes[edge.source].y + HEADER + ROW * 1.5)).abs() < ROW / 2.0
            );
            assert!(
                (route.last().unwrap().1 - (graph.nodes[edge.target].y + HEADER + ROW * 0.5)).abs()
                    < ROW / 2.0
            );
            for segment in route.windows(2) {
                for node in [&graph.nodes[edge.source], &graph.nodes[edge.target]] {
                    let ((x1, y1), (x2, y2)) = (segment[0], segment[1]);
                    let intersects = if x1 == x2 {
                        x1 > node.x
                            && x1 < node.x + node.width
                            && y1.max(y2) > node.y
                            && y1.min(y2) < node.y + node.height
                    } else {
                        y1 > node.y
                            && y1 < node.y + node.height
                            && x1.max(x2) > node.x
                            && x1.min(x2) < node.x + node.width
                    };
                    assert!(!intersects, "edge {i} intersects {}", node.table);
                }
            }
        }
        for i in 0..graph.edges.len() {
            for j in i + 1..graph.edges.len() {
                for a in graph.route(i).windows(2) {
                    for b in graph.route(j).windows(2) {
                        let horizontal_overlap = a[0].1 == a[1].1
                            && b[0].1 == b[1].1
                            && a[0].1 == b[0].1
                            && a[0].0.min(a[1].0).max(b[0].0.min(b[1].0))
                                < a[0].0.max(a[1].0).min(b[0].0.max(b[1].0));
                        let vertical_overlap = a[0].0 == a[1].0
                            && b[0].0 == b[1].0
                            && a[0].0 == b[0].0
                            && a[0].1.min(a[1].1).max(b[0].1.min(b[1].1))
                                < a[0].1.max(a[1].1).min(b[0].1.max(b[1].1));
                        assert!(
                            !horizontal_overlap && !vertical_overlap,
                            "links {i} and {j} share a segment"
                        );
                    }
                }
            }
        }
        assert!(!state.center_initial_view(&graph, gpui::size(gpui::px(0.0), gpui::px(0.0))));
        assert!(state.center_initial_view(&graph, gpui::size(gpui::px(800.0), gpui::px(600.0))));
        let offset = state.scroll.offset();
        assert!(offset.x < gpui::px(0.0) && offset.y < gpui::px(0.0));
        state.selected_edge = Some(0);
        assert!(std::sync::Arc::ptr_eq(
            &graph,
            &state.graph(base, "s", "t00", |value, size, _| value.len() as f32 * size)
        ));
        assert_eq!(state.selected_edge, Some(0));
        assert!(!state.center_initial_view(&graph, gpui::size(gpui::px(900.0), gpui::px(700.0))));
        assert_eq!(state.scroll.offset(), offset);
    }

    #[test]
    fn focused_graph_excludes_unrelated_and_transitive_tables() {
        let tables: Vec<_> = ["root", "parent", "child", "unrelated", "grandparent"]
            .into_iter()
            .map(|name| TableSummary {
                schema: "main".into(),
                name: name.into(),
                table_type: "TABLE".into(),
                estimated_rows: None,
            })
            .collect();
        let columns = tables
            .iter()
            .map(|t| {
                (
                    (t.schema.clone(), t.name.clone()),
                    vec![
                        ColumnMeta::new("id", DataType::Int4),
                        ColumnMeta::new("parent_id", DataType::Int4),
                    ],
                )
            })
            .collect();
        let key = |source: &str, target: &str| ForeignKey {
            name: format!("{source}_fk"),
            source_schema: "main".into(),
            source_table: source.into(),
            source_column: "parent_id".into(),
            target_schema: "main".into(),
            target_table: target.into(),
            target_column: "id".into(),
        };
        let graph = DiagramModel::build(
            &tables,
            &columns,
            vec![
                key("root", "parent"),
                key("child", "root"),
                key("parent", "grandparent"),
            ],
            None,
        )
        .focused("main", "root");
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 2);
        let node = |name: &str| graph.nodes.iter().find(|n| n.table == name).unwrap();
        assert!(node("child").x + node("child").width < node("root").x);
        assert!(node("root").x + node("root").width < node("parent").x);
        let isolated = DiagramModel::build(&tables, &columns, vec![], None).focused("main", "root");
        assert_eq!(isolated.nodes.len(), 1);
    }

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
        assert!(model.mapped_column(Some(0), model.edges[0].source, 0));
        assert!(!model.mapped_column(None, model.edges[0].source, 0));
        assert!(!model.mapped_column(Some(999), model.edges[0].source, 0));
        let focused = model.focused("a", "same");
        assert_eq!(focused.edges.len(), 4);
        assert_eq!(focused.nodes.len(), 3);
        let route = focused.route(0);
        assert!(
            route[1].0
                > focused.nodes[focused.edges[0].source].x
                    + focused.nodes[focused.edges[0].source].width
        );
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
