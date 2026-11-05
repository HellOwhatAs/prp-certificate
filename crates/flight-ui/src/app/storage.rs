use super::FlightApp;
use crate::platform;
use crate::theme::{BACKGROUND, INK, MUTED, RED, SURFACE};
use crate::widgets;
use eframe::egui::{self, Color32, RichText};

impl FlightApp {
    pub(super) fn request_storage(&mut self, operation: &str, q: Option<usize>, ctx: &egui::Context) {
        if self.storage_busy || (operation != "list" && self.busy()) {
            return;
        }
        self.storage_busy = true;
        self.storage_error.clear();
        platform::storage(operation, q, self.file_events.clone(), ctx.clone());
    }
    pub(super) fn storage_button(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(40.0, 44.0), egui::Sense::click());
        let response = widgets::icon_button(ui, response, "存储管理", self.storage_open);
        let center = rect.center();
        // Draw the gear directly so the icon does not depend on font coverage.
        let points: Vec<_> = (0..32)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / 32.0;
                let radius = if i % 4 == 0 || i % 4 == 3 { 8.0 } else { 10.5 };
                center + egui::vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        ui.painter().add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.6_f32, INK),
        ));
        ui.painter()
            .circle_stroke(center, 3.4, egui::Stroke::new(1.6_f32, INK));
        if response.clicked() {
            self.storage_open = !self.storage_open;
            if self.storage_open {
                self.request_storage("list", None, ui.ctx());
            }
        }
    }
    pub(super) fn storage_window(&mut self, ctx: &egui::Context) {
        if !self.storage_open {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("storage-management"))
            .backdrop_color(Color32::from_black_alpha(60))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(SURFACE)
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(230.0, 450.0));
                let (header, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 44.0),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    header.center(),
                    egui::Align2::CENTER_CENTER,
                    "存储管理",
                    egui::FontId::proportional(24.0),
                    INK,
                );
                let close_rect = egui::Rect::from_min_size(
                    egui::pos2(header.right() - 44.0, header.top()),
                    egui::vec2(44.0, 44.0),
                );
                let response = ui.interact(
                    close_rect,
                    egui::Id::new("storage-close"),
                    egui::Sense::click(),
                );
                let response = widgets::icon_button(ui, response, "关闭存储管理", false);
                for direction in [-1.0, 1.0] {
                    let offset = egui::vec2(6.0, direction * 6.0);
                    ui.painter().line_segment(
                        [close_rect.center() - offset, close_rect.center() + offset],
                        egui::Stroke::new(1.6_f32, INK),
                    );
                }
                close = response.clicked();
                ui.add_space(8.0);
                if self.storage_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("处理中…");
                    });
                    return;
                }
                if let Some(state) = self.storage_state.clone() {
                    if state["available"] == false {
                        ui.colored_label(MUTED, "浏览器未开放缓存存储。");
                    }
                    if let Some(rows) = state["rows"].as_array() {
                        for row in rows {
                            let q = row["q"].as_u64().unwrap_or(1) as usize;
                            let bytes = row["bytes"].as_u64().unwrap_or(0);
                            let cached = row["cached_bytes"].as_u64().unwrap_or(0);
                            let total = row["total_bytes"].as_u64().unwrap_or(0);
                            let card = widgets::card_frame().fill(BACKGROUND).inner_margin(14);
                            let content_width =
                                (ui.available_width() - card.total_margin().sum().x).max(0.0);
                            card.show(ui, |ui| {
                                ui.set_min_width(content_width);
                                ui.horizontal(|ui| {
                                    let text_width =
                                        (ui.available_width() - 64.0 - ui.spacing().item_spacing.x)
                                            .max(0.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(text_width, 44.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_min_width(text_width);
                                            ui.spacing_mut().item_spacing.y = 4.0;
                                            ui.label(
                                                RichText::new(
                                                    ["第一问", "第二问", "第三问"]
                                                        [q.saturating_sub(1).min(2)],
                                                )
                                                .strong(),
                                            );
                                            let size = if bytes == 0 {
                                                "未缓存".to_owned()
                                            } else if total > 0 && cached == total {
                                                format!("已缓存 {:.1} MB", cached as f64 / 1e6)
                                            } else {
                                                format!(
                                                    "部分缓存 {:.1} / {:.1} MB",
                                                    cached as f64 / 1e6,
                                                    total as f64 / 1e6
                                                )
                                            };
                                            ui.label(RichText::new(size).size(14.0).color(MUTED));
                                        },
                                    );
                                    if ui
                                        .add_enabled(
                                            bytes > 0 && !self.busy() && !self.storage_busy,
                                            egui::Button::new("删除")
                                                .min_size(egui::vec2(64.0, 36.0)),
                                        )
                                        .clicked()
                                    {
                                        self.request_storage("delete", Some(q), ctx);
                                    }
                                });
                            });
                        }
                    }
                }
                if !self.storage_error.is_empty() {
                    ui.colored_label(RED, &self.storage_error);
                    if ui
                        .add_enabled(
                            !self.storage_busy && !self.busy(),
                            egui::Button::new("重试"),
                        )
                        .clicked()
                    {
                        self.request_storage("list", None, ctx);
                    }
                }
            });
        if close || modal.should_close() {
            self.storage_open = false;
        }
    }
}
