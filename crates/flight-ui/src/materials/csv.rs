use std::{borrow::Cow, collections::HashMap};

use eframe::egui::{self, Color32, FontId, Painter, Rect, Sense, Stroke, Vec2};

const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
const INK: Color32 = Color32::from_rgb(29, 54, 59);
const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
const RULE: Color32 = Color32::from_rgb(224, 233, 229);
const CELL_PADDING: f32 = 12.0;
const MIN_COLUMN_WIDTH: f32 = 76.0;
const ROW_NUMBER_PADDING: f32 = 8.0;

/// Parsed data and browsing positions belong to the current application session.
#[derive(Default)]
pub(super) struct CsvViewer {
    files: HashMap<&'static str, Result<CachedCsv, String>>,
}

struct CsvData {
    headers: csv::StringRecord,
    rows: Vec<csv::StringRecord>,
}

struct CachedCsv {
    data: CsvData,
    layout: Option<TableLayout>,
    scroll_offset: Vec2,
}

struct TableLayout {
    font: FontId,
    number_font: FontId,
    pixels_per_point: f32,
    column_edges: Vec<f32>,
    gutter_width: f32,
    row_height: f32,
}

impl CsvViewer {
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, id: &'static str, bytes: &'static [u8]) {
        let cached = self.files.entry(id).or_insert_with(|| {
            parse_csv(bytes).map(|data| CachedCsv {
                data,
                layout: None,
                scroll_offset: Vec2::ZERO,
            })
        });
        let file = match cached {
            Ok(file) => file,
            Err(message) => {
                ui.colored_label(MUTED, message.as_str());
                return;
            }
        };

        ui.scope(|ui| {
            // Fixed row spacing is required for show_rows to skip off-screen records.
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let font = egui::TextStyle::Body.resolve(ui.style());
            let pixels_per_point = ui.ctx().pixels_per_point();
            if file.layout.as_ref().is_none_or(|layout| {
                layout.font != font || layout.pixels_per_point != pixels_per_point
            }) {
                file.layout = Some(TableLayout::new(ui, &file.data, font, pixels_per_point));
            }
            let layout = file.layout.as_ref().expect("CSV layout initialized");
            let available = ui.available_size();
            let header_height = (layout.row_height + 4.0).min(available.y.max(0.0));
            let (header_rect, _) =
                ui.allocate_exact_size(egui::vec2(available.x, header_height), Sense::hover());

            let output = egui::ScrollArea::both()
                .id_salt(("materials-csv", id))
                .auto_shrink([false, false])
                .min_scrolled_width(0.0)
                .min_scrolled_height(0.0)
                .max_height((available.y - header_height).max(0.0))
                .scroll_offset(file.scroll_offset)
                .show_rows(ui, layout.row_height, file.data.rows.len(), |ui, rows| {
                    let content_width = layout.gutter_width + layout.width();
                    ui.set_min_width(content_width);
                    let content_left = ui.max_rect().left();
                    let mut data_clip = ui.clip_rect();
                    data_clip.min.x = (header_rect.left() + layout.gutter_width)
                        .max(data_clip.left())
                        .min(data_clip.right());
                    let painter = ui.painter().with_clip_rect(data_clip);
                    let mut visible_rows = Vec::new();
                    for row_index in rows {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(content_width, layout.row_height),
                            Sense::hover(),
                        );
                        if !rect.intersects(ui.clip_rect()) {
                            continue;
                        }
                        visible_rows.push((row_index, rect));
                        let data_rect = Rect::from_min_max(
                            rect.min + egui::vec2(layout.gutter_width, 0.0),
                            rect.max,
                        );
                        let background = if row_index % 2 == 0 {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(250, 252, 251)
                        };
                        painter.rect_filled(data_rect, 0.0, background);
                        paint_record(&painter, data_rect, &file.data.rows[row_index], layout, INK);
                    }
                    // Reserve the gutter above cells but below ScrollArea's floating bars.
                    let gutter_shape = ui.painter().add(egui::Shape::Noop);
                    (content_left, visible_rows, gutter_shape)
                });
            file.scroll_offset = output.state.offset;
            let (content_left, visible_rows, gutter_shape) = output.inner;
            let gutter_rect = Rect::from_min_max(
                output.inner_rect.min,
                egui::pos2(
                    (output.inner_rect.left() + layout.gutter_width)
                        .min(output.inner_rect.right()),
                    output.inner_rect.bottom(),
                ),
            );
            let gutter_painter = ui
                .painter()
                .with_clip_rect(output.inner_rect.intersect(ui.clip_rect()));
            let mut gutter = Vec::with_capacity(visible_rows.len() + 2);
            gutter.push(egui::Shape::rect_filled(
                gutter_rect,
                0.0,
                super::CONTENT_HEADER_BACKGROUND,
            ));
            for (row_index, rect) in visible_rows {
                let number = gutter_painter.layout_no_wrap(
                    (row_index + 1).to_string(),
                    layout.number_font.clone(),
                    MUTED,
                );
                let position = egui::pos2(
                    gutter_rect.right() - ROW_NUMBER_PADDING - number.size().x,
                    rect.center().y - number.size().y / 2.0,
                );
                gutter.push(egui::Shape::galley(position, number, MUTED));
            }
            gutter.push(egui::Shape::line_segment(
                [gutter_rect.right_top(), gutter_rect.right_bottom()],
                Stroke::new(1.0_f32, RULE),
            ));
            // Pure paint keeps wheel and touch scrolling active over the row numbers.
            gutter_painter.set(gutter_shape, egui::Shape::Vec(gutter));

            // Paint after the body so its scrolling clip margin cannot bleed into the header.
            ui.painter()
                .rect_filled(header_rect, 0.0, super::CONTENT_HEADER_BACKGROUND);
            let header_clip = Rect::from_min_max(
                egui::pos2(gutter_rect.right(), header_rect.top()),
                egui::pos2(output.inner_rect.right(), header_rect.bottom()),
            )
            .intersect(header_rect)
            .intersect(ui.clip_rect());
            let painter = ui.painter().with_clip_rect(header_clip);
            // Use this frame's rendered origin: ScrollArea's returned offset may
            // already include wheel input that will move the rows on the next frame.
            let scrolled_header = Rect::from_min_size(
                egui::pos2(content_left + layout.gutter_width, header_rect.top()),
                egui::vec2(layout.width(), header_rect.height()),
            );
            paint_record(
                &painter,
                scrolled_header,
                &file.data.headers,
                layout,
                ACCENT,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(gutter_rect.right(), header_rect.top()),
                    egui::pos2(gutter_rect.right(), header_rect.bottom()),
                ],
                Stroke::new(1.0_f32, RULE),
            );
            ui.painter().line_segment(
                [header_rect.left_bottom(), header_rect.right_bottom()],
                Stroke::new(1.0_f32, RULE),
            );
        });
    }
}

impl TableLayout {
    fn new(ui: &egui::Ui, data: &CsvData, font: FontId, pixels_per_point: f32) -> Self {
        let mut widths = vec![MIN_COLUMN_WIDTH; data.headers.len()];
        let number_font = FontId::monospace((font.size - 2.0).max(12.0));
        let gutter_width = (ui
            .painter()
            .layout_no_wrap(data.rows.len().max(1).to_string(), number_font.clone(), MUTED)
            .size()
            .x
            + 2.0 * ROW_NUMBER_PADDING)
            .ceil();
        // Repeated location IDs and distance values only need measuring once.
        let mut measured: HashMap<&str, f32> = HashMap::new();
        ui.fonts_mut(|fonts| {
            for record in std::iter::once(&data.headers).chain(&data.rows) {
                for (column, value) in record.iter().enumerate() {
                    let text_width = *measured.entry(value).or_insert_with(|| {
                        fonts
                            .layout_no_wrap(single_line(value).into_owned(), font.clone(), INK)
                            .size()
                            .x
                    });
                    widths[column] = widths[column].max(text_width + 2.0 * CELL_PADDING);
                }
            }
        });
        let mut column_edges = Vec::with_capacity(widths.len() + 1);
        column_edges.push(0.0);
        for width in widths {
            column_edges.push(column_edges.last().copied().unwrap_or(0.0) + width.ceil());
        }
        Self {
            row_height: font.size + 16.0,
            font,
            number_font,
            pixels_per_point,
            column_edges,
            gutter_width,
        }
    }

    fn width(&self) -> f32 {
        self.column_edges.last().copied().unwrap_or(0.0)
    }
}

fn paint_record(
    painter: &Painter,
    rect: Rect,
    record: &csv::StringRecord,
    layout: &TableLayout,
    color: Color32,
) {
    for (column, value) in record.iter().enumerate() {
        let left = rect.left() + layout.column_edges[column];
        let right = rect.left() + layout.column_edges[column + 1];
        if right < painter.clip_rect().left() || left > painter.clip_rect().right() {
            continue;
        }
        painter.text(
            egui::pos2(left + CELL_PADDING, rect.center().y),
            egui::Align2::LEFT_CENTER,
            single_line(value),
            layout.font.clone(),
            color,
        );
        painter.line_segment(
            [
                egui::pos2(right, rect.top()),
                egui::pos2(right, rect.bottom()),
            ],
            Stroke::new(1.0_f32, RULE),
        );
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0_f32, RULE),
    );
}

fn parse_csv(bytes: &[u8]) -> Result<CsvData, String> {
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|error| format!("无法读取 CSV：{error}"))?
        .clone();
    if headers.is_empty() {
        return Err("CSV 文件为空。".into());
    }
    let rows = reader
        .records()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取 CSV：{error}"))?;
    Ok(CsvData { headers, rows })
}

// Keep records unchanged, but make embedded line breaks visible in fixed-height cells.
fn single_line(value: &str) -> Cow<'_, str> {
    if value.contains(['\r', '\n', '\t']) {
        Cow::Owned(
            value
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\n', " ↵ ")
                .replace('\t', " ⇥ "),
        )
    } else {
        Cow::Borrowed(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEOPLE_Q1: &[u8] = include_bytes!("../../../../public/data/peopleQ1.csv");
    const PEOPLE_Q2: &[u8] = include_bytes!("../../../../public/data/peopleQ2.csv");
    const PEOPLE_Q3: &[u8] = include_bytes!("../../../../public/data/peopleQ3.csv");
    const DISTANCES: &[u8] = include_bytes!("../../../../public/data/distances.csv");

    #[test]
    fn csv_preserves_bom_quoted_fields_empty_values_and_original_number_text() {
        let data = parse_csv(
            b"\xef\xbb\xbfid,note,value\r\n001,\"a,b\",1.00\r\n002,\"say \"\"hi\"\"\nnext line\",\r\n",
        )
        .unwrap();
        assert_eq!(data.headers.get(0), Some("id"));
        assert_eq!(data.rows.len(), 2);
        assert_eq!(
            data.rows[0].iter().collect::<Vec<_>>(),
            ["001", "a,b", "1.00"]
        );
        assert_eq!(data.rows[1].get(1), Some("say \"hi\"\nnext line"));
        assert_eq!(data.rows[1].get(2), Some(""));
    }

    #[test]
    fn every_public_data_file_is_parsed_completely() {
        for (name, bytes, expected_rows, expected_columns, last_id) in [
            ("peopleQ1.csv", PEOPLE_Q1, 1600, 3, "P3998"),
            ("peopleQ2.csv", PEOPLE_Q2, 4000, 3, "P4000"),
            ("peopleQ3.csv", PEOPLE_Q3, 4000, 6, "P4000"),
            ("distances.csv", DISTANCES, 55, 56, "F052"),
        ] {
            let data = parse_csv(bytes).unwrap();
            assert_eq!(data.headers.len(), expected_columns, "{name}");
            assert_eq!(data.rows.len(), expected_rows, "{name}");
            assert!(data.rows.iter().all(|row| row.len() == expected_columns));
            assert_eq!(data.rows.last().unwrap().get(0), Some(last_id), "{name}");
        }
    }

    #[test]
    fn invalid_csv_is_reported_and_header_only_files_remain_readable() {
        assert!(parse_csv(b"id,value\n1,2\n3\n").is_err());
        assert!(parse_csv(b"id,value\n1,\xff\n").is_err());
        assert!(parse_csv(b"").is_err());
        let data = parse_csv(b"id,value\n").unwrap();
        assert_eq!(data.headers.len(), 2);
        assert!(data.rows.is_empty());
    }

    fn draw(
        viewer: &mut CsvViewer,
        ctx: &egui::Context,
        id: &'static str,
        bytes: &'static [u8],
    ) -> egui::FullOutput {
        draw_with_events(viewer, ctx, id, bytes, 640.0, vec![])
    }

    fn draw_with_events(
        viewer: &mut CsvViewer,
        ctx: &egui::Context,
        id: &'static str,
        bytes: &'static [u8],
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 360.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| viewer.ui(ui, id, bytes));
            },
        )
    }

    fn text_rects(output: &egui::FullOutput) -> Vec<(&str, Rect, Rect)> {
        fn collect<'a>(
            shape: &'a egui::Shape,
            clip: Rect,
            result: &mut Vec<(&'a str, Rect, Rect)>,
        ) {
            match shape {
                egui::Shape::Text(text) => result.push((
                    text.galley.text(),
                    Rect::from_min_size(text.pos, text.galley.size()),
                    clip,
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, result);
                    }
                }
                _ => {}
            }
        }
        let mut result = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, shape.clip_rect, &mut result);
        }
        result
    }

    fn painted_text(output: &egui::FullOutput) -> Vec<&str> {
        text_rects(output)
            .into_iter()
            .map(|(text, _, _)| text)
            .collect()
    }

    fn label(output: &egui::FullOutput, expected: &str) -> (Rect, Rect) {
        text_rects(output)
            .into_iter()
            .find_map(|(text, rect, clip)| (text == expected).then_some((rect, clip)))
            .unwrap_or_else(|| panic!("missing table label: {expected}"))
    }

    fn wheel(position: egui::Pos2, delta: Vec2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn large_table_virtualizes_rows_and_retains_each_files_scroll_position() {
        let ctx = egui::Context::default();
        let mut viewer = CsvViewer::default();
        let first = draw(&mut viewer, &ctx, "q2", PEOPLE_Q2);
        let text = painted_text(&first);
        assert!(text.contains(&"P0001"));
        assert!(text.contains(&"1"));
        assert!(!text.contains(&"0"));
        assert!(
            (label(&first, "1").0.center().y - label(&first, "P0001").0.center().y).abs()
                < 0.1
        );
        assert!(text.len() < 100, "only visible records should be painted");

        let file = viewer.files.get_mut("q2").unwrap().as_mut().unwrap();
        file.scroll_offset.y = file.layout.as_ref().unwrap().row_height * 4000.0;
        draw(&mut viewer, &ctx, "q2", PEOPLE_Q2);
        let last = draw(&mut viewer, &ctx, "q2", PEOPLE_Q2);
        let text = painted_text(&last);
        assert!(
            text.contains(&"P4000"),
            "the final record must be reachable"
        );
        assert!(!text.contains(&"P0001"));
        assert!(text.contains(&"4000"));
        assert!(!text.contains(&"4001"));
        assert!(
            (label(&last, "4000").0.center().y - label(&last, "P4000").0.center().y).abs()
                < 0.1
        );
        assert!(text.len() < 100);
        let q2_offset = viewer.files["q2"].as_ref().unwrap().scroll_offset;

        let other = draw(&mut viewer, &ctx, "q1", PEOPLE_Q1);
        assert!(painted_text(&other).contains(&"P0002"));
        assert!(
            (label(&other, "1").0.center().y - label(&other, "P0002").0.center().y).abs()
                < 0.1
        );
        let restored = draw(&mut viewer, &ctx, "q2", PEOPLE_Q2);
        assert!(painted_text(&restored).contains(&"P4000"));
        assert_eq!(
            viewer.files["q2"].as_ref().unwrap().scroll_offset,
            q2_offset
        );

        let (_, body_clip) = label(&restored, "4000");
        let gutter_width = viewer.files["q2"]
            .as_ref()
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .gutter_width;
        let over_gutter = egui::pos2(body_clip.left() + gutter_width / 2.0, body_clip.center().y);
        draw_with_events(
            &mut viewer,
            &ctx,
            "q2",
            PEOPLE_Q2,
            640.0,
            wheel(over_gutter, egui::vec2(0.0, 120.0)),
        );
        assert!(
            viewer.files["q2"].as_ref().unwrap().scroll_offset.y < q2_offset.y,
            "wheel input over the fixed row numbers must scroll the table"
        );
    }

    #[test]
    fn horizontal_scrolling_keeps_row_numbers_fixed_and_headers_aligned() {
        let ctx = egui::Context::default();
        let mut viewer = CsvViewer::default();
        let draw_table = |viewer: &mut CsvViewer, events| {
            draw_with_events(viewer, &ctx, "q3", PEOPLE_Q3, 640.0, events)
        };
        for _ in 0..3 {
            draw_table(&mut viewer, vec![]);
        }
        let first = draw_table(&mut viewer, vec![]);
        let (number, body_clip) = label(&first, "1");
        let layout = viewer.files["q3"]
            .as_ref()
            .unwrap()
            .layout
            .as_ref()
            .unwrap();
        let gutter_right = body_clip.left() + layout.gutter_width;
        let content_width = layout.gutter_width + layout.width();
        let first_column_x = label(&first, "earliest_pickup_time").0.left();
        let over_gutter = egui::pos2(
            (body_clip.left() + gutter_right) / 2.0,
            number.center().y,
        );
        let wheel_frame = draw_table(
            &mut viewer,
            wheel(over_gutter, egui::vec2(-160.0, 0.0)),
        );
        assert!(viewer.files["q3"].as_ref().unwrap().scroll_offset.x > 0.0);
        let next_frame = draw_table(&mut viewer, vec![]);
        for frame in [&wheel_frame, &next_frame] {
            assert!((label(frame, "1").0.right() - number.right()).abs() < 0.1);
            for (header, value) in [
                ("earliest_pickup_time", "2026-08-05 08:33"),
                ("latest_arrival_time", "2026-08-06 09:40"),
            ] {
                let (heading, heading_clip) = label(frame, header);
                let (cell, cell_clip) = label(frame, value);
                assert!(
                    (heading.left() - cell.left()).abs() < 0.1,
                    "header and data must use the same frame's scroll origin"
                );
                assert!(heading_clip.left() >= gutter_right);
                assert!(cell_clip.left() >= gutter_right);
            }
        }
        assert!(label(&next_frame, "earliest_pickup_time").0.left() < first_column_x);

        let draw_narrow = |viewer: &mut CsvViewer| {
            draw_with_events(viewer, &ctx, "q3", PEOPLE_Q3, 320.0, vec![])
        };
        viewer.files.get_mut("q3").unwrap().as_mut().unwrap().scroll_offset.x = 0.0;
        let narrow_start = draw_narrow(&mut viewer);
        let narrow_number = label(&narrow_start, "1").0;
        let file = viewer.files.get_mut("q3").unwrap().as_mut().unwrap();
        file.scroll_offset.x = content_width;
        draw_narrow(&mut viewer);
        let right_edge = draw_narrow(&mut viewer);
        let (heading, clip) = label(&right_edge, "task_type");
        assert!(
            heading.right() <= clip.right(),
            "the final column must be fully reachable after reserving the gutter"
        );
        assert!((label(&right_edge, "1").0.right() - narrow_number.right()).abs() < 0.1);
    }
}
