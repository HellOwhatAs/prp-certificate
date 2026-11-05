use std::{borrow::Cow, collections::HashMap};

use eframe::egui::{self, Color32, FontId, Painter, Rect, Sense, Stroke, Vec2};

const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
const INK: Color32 = Color32::from_rgb(29, 54, 59);
const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
const RULE: Color32 = Color32::from_rgb(224, 233, 229);
const CELL_PADDING: f32 = 12.0;
const MIN_COLUMN_WIDTH: f32 = 76.0;

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
    pixels_per_point: f32,
    column_edges: Vec<f32>,
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
                    ui.set_min_width(layout.width());
                    let content_left = ui.max_rect().left();
                    for row_index in rows {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(layout.width(), layout.row_height),
                            Sense::hover(),
                        );
                        if !rect.intersects(ui.clip_rect()) {
                            continue;
                        }
                        let background = if row_index % 2 == 0 {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(250, 252, 251)
                        };
                        ui.painter().rect_filled(rect, 0.0, background);
                        paint_record(ui.painter(), rect, &file.data.rows[row_index], layout, INK);
                    }
                    content_left
                });
            file.scroll_offset = output.state.offset;

            // Paint after the body so its scrolling clip margin cannot bleed into the header.
            ui.painter()
                .rect_filled(header_rect, 0.0, super::CONTENT_HEADER_BACKGROUND);
            let header_clip = Rect::from_min_max(
                egui::pos2(output.inner_rect.left(), header_rect.top()),
                egui::pos2(output.inner_rect.right(), header_rect.bottom()),
            )
            .intersect(header_rect)
            .intersect(ui.clip_rect());
            let painter = ui.painter().with_clip_rect(header_clip);
            // Use this frame's rendered origin: ScrollArea's returned offset may
            // already include wheel input that will move the rows on the next frame.
            let scrolled_header = Rect::from_min_size(
                egui::pos2(output.inner, header_rect.top()),
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
                [header_rect.left_bottom(), header_rect.right_bottom()],
                Stroke::new(1.0_f32, RULE),
            );
        });
    }
}

impl TableLayout {
    fn new(ui: &egui::Ui, data: &CsvData, font: FontId, pixels_per_point: f32) -> Self {
        let mut widths = vec![MIN_COLUMN_WIDTH; data.headers.len()];
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
            pixels_per_point,
            column_edges,
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
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 360.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| viewer.ui(ui, id, bytes));
            },
        )
    }

    fn painted_text(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn large_table_virtualizes_rows_and_retains_each_files_scroll_position() {
        let ctx = egui::Context::default();
        let mut viewer = CsvViewer::default();
        let first = draw(&mut viewer, &ctx, "q2", PEOPLE_Q2);
        let text = painted_text(&first);
        assert!(text.contains(&"P0001"));
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
        assert!(text.len() < 100);
        let q2_offset = viewer.files["q2"].as_ref().unwrap().scroll_offset;

        let other = draw(&mut viewer, &ctx, "q1", PEOPLE_Q1);
        assert!(painted_text(&other).contains(&"P0002"));
        let restored = draw(&mut viewer, &ctx, "q2", PEOPLE_Q2);
        assert!(painted_text(&restored).contains(&"P4000"));
        assert_eq!(
            viewer.files["q2"].as_ref().unwrap().scroll_offset,
            q2_offset
        );
    }
}
