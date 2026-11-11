use super::{CARD_PADDING, DiagramModel, HEADER, HEADER_FONT, NAME_FONT, ROW, single_line_label};
use std::io::{BufWriter, Write};
use std::path::Path;

const MARGIN: f32 = 16.0;

pub(super) fn write_svg(path: &Path, graph: &DiagramModel) -> anyhow::Result<()> {
    anyhow::ensure!(!graph.nodes.is_empty(), "No diagram to export");
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    {
        let mut writer = BufWriter::new(file.as_file_mut());
        render(&mut writer, graph)?;
        writer.flush()?;
    }
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn render(writer: &mut impl Write, graph: &DiagramModel) -> std::io::Result<()> {
    let (mut left, mut top, mut right, mut bottom) = (0.0_f32, 0.0_f32, graph.width, graph.height);
    for index in 0..graph.edges.len() {
        for &(x, y) in graph.route(index) {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    let x = left - MARGIN;
    let y = top - MARGIN;
    let width = right - left + 2.0 * MARGIN;
    let height = bottom - top + 2.0 * MARGIN;
    writeln!(
        writer,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="{x} {y} {width} {height}">"#
    )?;
    writeln!(writer, "<title>FastDB entity-relationship diagram</title>")?;
    if let Some(notice) = &graph.notice {
        write!(writer, "<desc>")?;
        xml_text(writer, notice)?;
        writeln!(writer, "</desc>")?;
    }
    writeln!(
        writer,
        r##"<defs><marker id="fk-arrow" markerUnits="userSpaceOnUse" markerWidth="12" markerHeight="14" refX="11" refY="7" orient="auto"><path d="M1 1L11 7L1 13" fill="none" stroke="#52647a" stroke-width="2"/></marker></defs>
<rect x="{x}" y="{y}" width="{width}" height="{height}" fill="#f4f7fb"/>"##
    )?;
    for (index, edge) in graph.edges.iter().enumerate() {
        write!(
            writer,
            r##"<polyline fill="none" stroke="#52647a" stroke-width="2" marker-end="url(#fk-arrow)" points=""##
        )?;
        for (vertex, (x, y)) in graph.route(index).iter().enumerate() {
            if vertex > 0 {
                write!(writer, " ")?;
            }
            write!(writer, "{x},{y}")?;
        }
        write!(writer, "\"><title>")?;
        xml_text(writer, &edge.fk.name)?;
        writeln!(writer, "</title></polyline>")?;
    }
    writeln!(
        writer,
        r##"<g fill="#17283f" font-family="system-ui, sans-serif">"##
    )?;
    for node in &graph.nodes {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        writeln!(
            writer,
            r##"<rect x="{x}" y="{y}" width="{width}" height="{height}" rx="5" fill="#ffffff" stroke="#52647a"/>"##
        )?;
        let text_x = x + CARD_PADDING;
        let heading =
            single_line_label(&node.schema).into_owned() + "." + &single_line_label(&node.table);
        label(
            writer,
            text_x,
            y + HEADER / 2.0,
            HEADER_FONT,
            true,
            &heading,
        )?;
        if node.columns.is_empty() {
            label(
                writer,
                text_x,
                y + HEADER + ROW / 2.0,
                NAME_FONT,
                false,
                "Columns unavailable",
            )?;
        }
        for (index, column) in node.columns.iter().enumerate() {
            let text_y = y + HEADER + (index as f32 + 0.5) * ROW;
            label(
                writer,
                text_x,
                text_y,
                NAME_FONT,
                false,
                &single_line_label(&column.name),
            )?;
        }
    }
    writeln!(writer, "</g></svg>")
}

fn label(
    writer: &mut impl Write,
    x: f32,
    y: f32,
    size: f32,
    bold: bool,
    text: &str,
) -> std::io::Result<()> {
    let weight = if bold { "bold" } else { "normal" };
    write!(
        writer,
        r#"<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" dominant-baseline="middle">"#
    )?;
    xml_text(writer, text)?;
    writeln!(writer, "</text>")
}

fn xml_text(writer: &mut impl Write, text: &str) -> std::io::Result<()> {
    for ch in text.chars() {
        match ch {
            '&' => writer.write_all(b"&amp;")?,
            '<' => writer.write_all(b"&lt;")?,
            '>' => writer.write_all(b"&gt;")?,
            '\u{0}'..='\u{8}'
            | '\u{B}'
            | '\u{C}'
            | '\u{E}'..='\u{1F}'
            | '\u{FFFE}'
            | '\u{FFFF}' => {
                write!(writer, "�")?;
            }
            _ => write!(writer, "{ch}")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::write_svg;
    use crate::workspace::diagram_model::{DiagramModel, DiagramState};
    use db_wire::{ColumnMeta, DataType, ForeignKey};
    use std::{collections::HashMap, sync::Arc};

    #[test]
    fn svg_contains_escaped_columns_and_the_focused_fk_route() {
        let key = ForeignKey {
            name: "<&fk>".into(),
            source_schema: "app".into(),
            source_table: "orders".into(),
            source_column: "user<&>id".into(),
            target_schema: "app".into(),
            target_table: "users".into(),
            target_column: "id".into(),
        };
        let base = Arc::new(DiagramModel::build(
            &[],
            &HashMap::from([
                (
                    ("app".into(), "orders".into()),
                    vec![ColumnMeta::new("user<&>id", DataType::Int8)],
                ),
                (
                    ("app".into(), "users".into()),
                    vec![ColumnMeta::new("id", DataType::Int8)],
                ),
            ]),
            vec![key],
            None,
        ));
        let mut state = DiagramState::default();
        let initial = state.graph(base.clone(), "app", "orders", |_, _, _| 0.0);
        let index = initial
            .nodes
            .iter()
            .position(|node| node.table == "orders")
            .unwrap();
        let card = &initial.nodes[index];
        state.drag = Some((
            index,
            gpui::point(gpui::px(0.0), gpui::px(0.0)),
            card.x,
            card.y,
        ));
        state.move_card(gpui::point(gpui::px(55.0), gpui::px(30.0)));
        let graph = state.graph(base, "app", "orders", |_, _, _| 0.0);
        assert_eq!(graph.nodes[index].x, card.x + 55.0);
        assert_eq!(graph.nodes[index].y, card.y + 30.0);
        assert_ne!(initial.route(0), graph.route(0));
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("schema.svg");
        std::fs::write(&output, "previous").unwrap();

        assert!(write_svg(&output, &DiagramModel::default()).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "previous");
        write_svg(&output, &graph).unwrap();
        let svg = std::fs::read_to_string(output).unwrap();
        let route = graph
            .route(0)
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()
            .join(" ");
        let moved = &graph.nodes[index];
        assert!(svg.contains(&format!(r#"<rect x="{}" y="{}""#, moved.x, moved.y)));
        assert!(svg.contains(&format!("points=\"{route}\"")));
        assert!(crate::workspace::value_format::pretty_xml(&svg).is_ok());
        assert!(svg.contains("marker-end=\"url(#fk-arrow)\""));
        assert!(svg.contains("&lt;&amp;fk&gt;"));
        assert!(svg.contains("user&lt;&amp;&gt;id"));
        assert!(svg.contains("app.orders"));
        assert!(svg.contains("app.users"));
        assert_eq!(svg.matches("<polyline ").count(), 1);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
