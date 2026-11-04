mod csv;
mod pdf;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
const INK: Color32 = Color32::from_rgb(29, 54, 59);
const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
const BORDER: Color32 = Color32::from_rgb(211, 222, 216);
const TAB_BACKGROUND: Color32 = Color32::from_rgb(237, 242, 239);
const ROW_HEIGHT: f32 = 44.0;

struct MaterialFile {
    name: &'static str,
    bytes: &'static [u8],
}

// Preview and download share the same original bytes on desktop and the web.
const FILES: [MaterialFile; 5] = [
    MaterialFile {
        name: "problem.pdf",
        bytes: include_bytes!("../../../../public/problem.pdf"),
    },
    MaterialFile {
        name: "distances.csv",
        bytes: include_bytes!("../../../../public/data/distances.csv"),
    },
    MaterialFile {
        name: "peopleQ1.csv",
        bytes: include_bytes!("../../../../public/data/peopleQ1.csv"),
    },
    MaterialFile {
        name: "peopleQ2.csv",
        bytes: include_bytes!("../../../../public/data/peopleQ2.csv"),
    },
    MaterialFile {
        name: "peopleQ3.csv",
        bytes: include_bytes!("../../../../public/data/peopleQ3.csv"),
    },
];

#[derive(Default)]
pub(crate) struct MaterialsPage {
    selected: usize,
    pdf: pdf::PdfViewer,
    csv: csv::CsvViewer,
}

impl MaterialsPage {
    /// A save request is returned synchronously to preserve browser user activation.
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) -> Option<(&'static str, &'static [u8])> {
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        ui.painter().rect_filled(rect, 3.0, Color32::WHITE);
        let inner = rect.shrink(1.0);
        let mut workspace = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("materials-workspace")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        workspace.set_clip_rect(ui.clip_rect().intersect(inner));
        workspace.spacing_mut().item_spacing = Vec2::ZERO;
        let save = self.file_bar(&mut workspace).clicked();
        if self.selected == 0 {
            self.pdf.ui(&mut workspace);
        } else {
            let file = &FILES[self.selected];
            self.csv.ui(&mut workspace, file.name, file.bytes);
        }
        ui.painter().rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        let file = &FILES[self.selected];
        save.then_some((file.name, file.bytes))
    }

    fn file_bar(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let (bar, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, TAB_BACKGROUND);
        ui.painter()
            .hline(bar.x_range(), bar.bottom(), Stroke::new(1.0_f32, BORDER));

        // Keep this action outside the scrolling strip, including on touch screens.
        let save_rect = Rect::from_min_max(
            egui::pos2((bar.right() - ROW_HEIGHT).max(bar.left()), bar.top()),
            bar.right_bottom(),
        );
        let tabs_rect = Rect::from_min_max(bar.min, save_rect.left_bottom());
        let mut tabs = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("material-tabs")
                .max_rect(tabs_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        tabs.set_clip_rect(ui.clip_rect().intersect(tabs_rect));
        egui::ScrollArea::horizontal()
            .id_salt("material-file-strip")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
            .max_height(ROW_HEIGHT)
            .show(&mut tabs, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let mut focus_next = None;
                        let mut responses = Vec::with_capacity(FILES.len());
                        for (index, file) in FILES.iter().enumerate() {
                            let response = file_tab(ui, file.name, self.selected == index);
                            if response.clicked() {
                                self.selected = index;
                                response.request_focus();
                                response.scroll_to_me(None);
                            }
                            if response.has_focus() && ui.is_enabled() {
                                let step = ui.input_mut(|input| {
                                    if input
                                        .consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                                    {
                                        Some((index + 1) % FILES.len())
                                    } else if input
                                        .consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
                                    {
                                        Some((index + FILES.len() - 1) % FILES.len())
                                    } else {
                                        None
                                    }
                                });
                                focus_next = focus_next.or(step);
                            }
                            responses.push(response);
                        }
                        if let Some(index) = focus_next {
                            self.selected = index;
                            responses[index].request_focus();
                            responses[index].scroll_to_me(None);
                        }
                    },
                );
            });

        ui.painter().vline(
            save_rect.left(),
            save_rect.y_range(),
            Stroke::new(1.0_f32, BORDER),
        );
        let response = ui.interact(save_rect, ui.id().with("save-material"), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(save_rect.shrink(5.0), 4.0, Color32::from_rgb(219, 233, 225));
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                save_rect.shrink(4.0),
                4.0,
                Stroke::new(1.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        save_icon(ui.painter(), save_rect.center());
        let label = format!("保存 {}", FILES[self.selected].name);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
        });
        response.on_hover_text(label)
    }
}

fn file_tab(ui: &mut egui::Ui, name: &str, selected: bool) -> egui::Response {
    let color = if selected { INK } else { MUTED };
    let text = ui
        .painter()
        .layout_no_wrap(name.into(), FontId::proportional(14.0), color);
    let size = egui::vec2(text.size().x + 48.0, ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Color32::WHITE
        } else if response.hovered() {
            Color32::from_rgb(226, 235, 230)
        } else {
            TAB_BACKGROUND
        };
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter()
            .vline(rect.right(), rect.y_range(), Stroke::new(1.0_f32, BORDER));
        if selected {
            ui.painter().rect_filled(
                Rect::from_min_size(rect.min, egui::vec2(rect.width(), 2.0)),
                0.0,
                ACCENT,
            );
        } else {
            ui.painter()
                .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0_f32, BORDER));
        }
        let center = egui::pos2(rect.left() + 19.0, rect.center().y);
        let stroke = Stroke::new(1.2_f32, if selected { ACCENT } else { MUTED });
        ui.painter().add(egui::Shape::line(
            vec![
                center + egui::vec2(1.0, -7.0),
                center + egui::vec2(-5.0, -7.0),
                center + egui::vec2(-5.0, 7.0),
                center + egui::vec2(5.0, 7.0),
                center + egui::vec2(5.0, -3.0),
                center + egui::vec2(1.0, -7.0),
                center + egui::vec2(1.0, -3.0),
                center + egui::vec2(5.0, -3.0),
            ],
            stroke,
        ));
        ui.painter().galley(
            egui::pos2(rect.left() + 33.0, rect.center().y - text.size().y / 2.0),
            text,
            color,
        );
        if response.has_focus() && !selected {
            ui.painter().rect_stroke(
                rect.shrink(4.0),
                2.0,
                Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
        }
    }
    response.on_hover_text(name)
}

fn save_icon(painter: &egui::Painter, center: Pos2) {
    let stroke = Stroke::new(1.6_f32, ACCENT);
    painter.line_segment(
        [
            center + egui::vec2(0.0, -8.0),
            center + egui::vec2(0.0, 3.0),
        ],
        stroke,
    );
    painter.add(egui::Shape::line(
        vec![
            center + egui::vec2(-4.0, -1.0),
            center + egui::vec2(0.0, 3.0),
            center + egui::vec2(4.0, -1.0),
        ],
        stroke,
    ));
    painter.add(egui::Shape::line(
        vec![
            center + egui::vec2(-7.0, 3.0),
            center + egui::vec2(-7.0, 8.0),
            center + egui::vec2(7.0, 8.0),
            center + egui::vec2(7.0, 3.0),
        ],
        stroke,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(pos: Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    #[test]
    fn narrow_file_strip_keeps_save_visible_and_keyboard_selection_reachable() {
        for width in [280.0, 390.0, 1440.0] {
            let ctx = egui::Context::default();
            ctx.style_mut(|style| style.animation_time = 0.0);
            let mut page = MaterialsPage::default();
            let frame = |page: &mut MaterialsPage, events| {
                let mut save = None;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(width, 100.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                save = Some(page.file_bar(ui));
                            });
                    },
                );
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && FILES.iter().any(|file| file.name == text.galley.text())
                        && shape.clip_rect.bottom() <= ROW_HEIGHT + 0.1
                    {
                        let text_rect = text.visual_bounding_rect();
                        assert!(
                            text_rect.top() >= 4.0 && text_rect.bottom() <= ROW_HEIGHT - 4.0,
                            "file tab must be vertically centered and unclipped: {text_rect:?}"
                        );
                    }
                }
                save.unwrap()
            };
            for _ in 0..3 {
                frame(&mut page, vec![]);
            }
            let save_rect = frame(&mut page, vec![]).rect;
            assert!((save_rect.right() - width).abs() < 0.1);
            assert_eq!(save_rect.width(), ROW_HEIGHT);
            frame(&mut page, click(egui::pos2(40.0, ROW_HEIGHT / 2.0)));
            frame(
                &mut page,
                vec![egui::Event::Key {
                    key: egui::Key::ArrowLeft,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(page.selected, 4, "last file must be reachable at {width}px");
            for _ in 0..3 {
                assert_eq!(frame(&mut page, vec![]).rect, save_rect);
            }
            assert!(frame(&mut page, click(save_rect.center())).clicked());
            assert_eq!(page.selected, 4);
        }
    }

    #[test]
    fn saving_after_a_file_switch_returns_that_files_original_bytes() {
        let ctx = egui::Context::default();
        let mut page = MaterialsPage {
            selected: 1,
            ..Default::default()
        };
        let frame = |page: &mut MaterialsPage, events| {
            let mut request = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1040.0, 600.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            request = page.ui(ui);
                        });
                },
            );
            (output, request)
        };
        for _ in 0..3 {
            frame(&mut page, vec![]);
        }
        let (output, _) = frame(&mut page, vec![]);
        let target = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "peopleQ2.csv" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("visible file tab");
        frame(&mut page, click(target));
        assert_eq!(page.selected, 3);
        let (_, request) = frame(&mut page, click(egui::pos2(1017.0, 23.0)));
        let (name, bytes) = request.expect("save current file");
        assert_eq!(name, "peopleQ2.csv");
        assert_eq!(
            bytes,
            include_bytes!("../../../../public/data/peopleQ2.csv")
        );
    }
}
