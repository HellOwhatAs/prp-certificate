use super::{FlightApp, NAMES, Page, Question, Review};
use crate::certificate::{self, Verified, fmt};
use crate::platform;
use crate::theme::{ACCENT, BORDER, CARD_RADIUS, INK, MUTED, RED, SURFACE};
use crate::widgets;
use eframe::egui::{self, RichText};

impl FlightApp {
    fn section(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) -> egui::Response {
        widgets::card_frame()
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                body(ui);
            })
            .response
    }
    fn question_card(&mut self, ui: &mut egui::Ui, q: usize) {
        Self::section(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(NAMES[q]).size(18.0).strong());
                let question = &self.questions[q];
                let state = if question.proof_for(&self.team).is_some() {
                    "已生成"
                } else if question.checked.is_some() {
                    "已校验"
                } else if !question.error.is_empty() {
                    "校验失败"
                } else if question.proof.is_some() {
                    "需重新生成"
                } else if question.active() {
                    "待校验"
                } else {
                    "未提交"
                };
                ui.label(RichText::new(state).size(13.0).color(MUTED));
            });
            self.file_button(ui, q, 0, "航线表");
            self.file_button(ui, q, 1, "人员分配表");
            ui.scope(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(10.0, 8.0);
                if ui.available_width() >= 364.0 {
                    ui.horizontal(|ui| {
                        self.template_buttons(ui, q);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.question_buttons(ui, q);
                        });
                    });
                } else {
                    ui.horizontal(|ui| self.template_buttons(ui, q));
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.question_buttons(ui, q);
                        });
                    });
                }
            });
            if !self.questions[q].error.is_empty() {
                ui.colored_label(RED, &self.questions[q].error);
            }
        });
    }
    fn template_buttons(&mut self, ui: &mut egui::Ui, q: usize) {
        for (label, assignment, width) in [("航线模板", false, 80.0), ("人员分配模板", true, 108.0)]
        {
            if ui
                .add_sized(
                    [width, 36.0],
                    egui::Button::new(RichText::new(label).size(14.0)),
                )
                .clicked()
            {
                self.template(q, assignment, ui.ctx());
            }
        }
    }
    fn question_buttons(&mut self, ui: &mut egui::Ui, q: usize) {
        // Right-to-left layout keeps the primary check button at the right edge.
        let check = widgets::primary_button(
            ui,
            self.questions[q].has_files() && !self.busy() && !self.storage_busy,
            [64.0, 36.0],
            RichText::new("校验").size(14.0),
        )
        .clicked();
        if check {
            self.check_question(q);
        }
        let clear = ui
            .add_enabled_ui(self.questions[q].active() && !self.busy(), |ui| {
                ui.add_sized(
                    [64.0, 36.0],
                    egui::Button::new(RichText::new("清空").size(14.0)),
                )
                .clicked()
            })
            .inner;
        if clear {
            self.questions[q] = Question::default();
            self.invalidate_generated();
        }
    }
    fn file_button(&mut self, ui: &mut egui::Ui, q: usize, slot: usize, label: &str) {
        ui.label(RichText::new(label).size(14.0).color(MUTED));
        let name = &self.questions[q].files[slot].name;
        let label = if name.is_empty() {
            "选择 CSV"
        } else {
            name.as_str()
        };
        if ui
            .add_enabled(
                !self.busy(),
                egui::Button::new(RichText::new(label).size(14.0))
                    .wrap()
                    .min_size(egui::vec2(ui.available_width(), 40.0)),
            )
            .clicked()
        {
            platform::pick(q * 2 + slot, self.file_events.clone(), ui.ctx().clone());
        }
    }
    fn form(&mut self, ui: &mut egui::Ui) {
        Self::section(ui, |ui| {
            ui.label("队伍署名");
            if ui
                .add_enabled(
                    !self.busy(),
                    egui::TextEdit::singleline(&mut self.team)
                        .id_salt("certificate-team-name")
                        .margin(egui::vec2(12.0, 10.0))
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                self.invalidate_generated();
            }
        });
        for q in 0..3 {
            ui.add_space(2.0);
            self.question_card(ui, q);
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let size = egui::vec2(
                (ui.available_width() - ui.spacing().item_spacing.x) / 2.0,
                46.0,
            );
            let generate = widgets::primary_button(
                ui,
                !self.busy() && !self.storage_busy && self.questions.iter().any(Question::active),
                size,
                "生成证书",
            )
            .clicked();
            if generate {
                self.generate_certificate(ui.ctx());
            }
            let export = ui
                .add_enabled_ui(self.ready.is_some() && !self.busy(), |ui| {
                    ui.add_sized(size, egui::Button::new("导出证书")).clicked()
                })
                .inner;
            if export {
                if let Some(item) = self.ready.clone() {
                    self.export(&item, ui.ctx());
                }
            }
        });
    }
    fn picture(ui: &mut egui::Ui, texture: Option<&egui::TextureHandle>) {
        if let Some(texture) = texture {
            let width = ui.available_width();
            let height = width * certificate::HEIGHT as f32 / certificate::WIDTH as f32;
            ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(width, height)));
        }
    }
    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        Self::picture(ui, self.draft_texture.as_ref());
    }
    pub(super) fn generate_page(&mut self, ui: &mut egui::Ui) {
        if ui.available_width() >= 900.0 {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                ui.columns(2, |cols| {
                    cols[0].spacing_mut().item_spacing.x = 12.0;
                    cols[1].spacing_mut().item_spacing.x = 12.0;
                    self.form(&mut cols[0]);
                    self.preview_panel(&mut cols[1]);
                });
            });
        } else {
            self.form(ui);
            ui.add_space(16.0);
            self.preview_panel(ui);
        }
    }
    fn verification_details(&self, ui: &mut egui::Ui) {
        if let Some(Review { name, item, .. }) = &self.review {
            Self::section(ui, |ui| {
                ui.label(RichText::new(item.team()).size(20.0).strong());
                ui.label(
                    RichText::new(format!("证书编号  {}", &item.id()[..16]))
                        .size(13.0)
                        .color(MUTED),
                );
                if let Some(name) = name {
                    ui.label(RichText::new(name).size(13.0).color(MUTED));
                }
            });
            for q in 1..=3 {
                Self::section(ui, |ui| {
                    let scores = item.score_for(q);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(NAMES[q - 1]).size(18.0).strong());
                        ui.label(
                            RichText::new(if scores.is_some() {
                                "验证通过"
                            } else {
                                "未提供"
                            })
                            .size(13.0)
                            .color(if scores.is_some() {
                                ACCENT
                            } else {
                                MUTED
                            }),
                        );
                    });
                    if let Some(scores) = scores {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        let font_size = if ui.available_width() < 280.0 {
                            14.0
                        } else {
                            16.0
                        };
                        let objectives = certificate::score_objectives(q, scores)
                            .into_iter()
                            .map(|(label, value, unit)| (label, format!("{value} {unit}")));
                        for (label, value) in objectives.chain(certificate::score_metrics(scores)) {
                            ui.columns(2, |cols| {
                                cols[0].label(RichText::new(label).size(font_size).color(MUTED));
                                cols[1].with_layout(
                                    egui::Layout::top_down(egui::Align::Max),
                                    |ui| {
                                        ui.label(RichText::new(value).size(font_size));
                                    },
                                );
                            });
                        }
                    }
                });
            }
        }
        for failure in &self.import_failures {
            Self::section(ui, |ui| {
                ui.label(RichText::new(&failure.name).strong());
                ui.colored_label(RED, "验证失败");
                ui.label(&failure.message);
            });
        }
    }
    fn review_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(Review { texture, .. }) = &self.review {
            Self::picture(ui, Some(texture));
        }
    }
    pub(super) fn verify_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("导入图片"))
                .clicked()
            {
                platform::pick(6, self.file_events.clone(), ui.ctx().clone());
            }
            if self.imports_paused && !self.pending.is_empty() && ui.button("继续验证").clicked()
            {
                self.imports_paused = false;
            }
        });
        ui.add_space(8.0);
        if ui.available_width() >= 900.0 {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                ui.columns(2, |cols| {
                    cols[0].spacing_mut().item_spacing.x = 12.0;
                    cols[1].spacing_mut().item_spacing.x = 12.0;
                    self.verification_details(&mut cols[0]);
                    self.review_panel(&mut cols[1]);
                });
            });
        } else {
            self.verification_details(ui);
            ui.add_space(16.0);
            self.review_panel(ui);
        }
    }
    pub(super) fn board_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("证书记录");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.records.is_empty() && !self.busy(),
                        egui::Button::new("清空").small(),
                    )
                    .clicked()
                {
                    self.confirm_clear = true;
                }
            });
        });
        if self.records.is_empty() {
            ui.label(RichText::new("暂无记录").color(MUTED));
            return;
        }
        let items = self.records.clone();
        if ui.available_width() >= 1020.0 {
            let width = ui.available_width();
            let flexible = width - 24.0 - 296.0 - 5.0 * 20.0;
            let columns = [
                flexible * 0.28,
                flexible * 0.18,
                flexible * 0.18,
                flexible * 0.18,
                flexible * 0.18,
                296.0,
            ];
            let (header, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::hover());
            let mut x = header.left() + 12.0;
            for (label, w) in [
                "队伍",
                "第一问 / 分钟",
                "第二问 / 分钟",
                "第三问 / 分钟",
                "临时任务 / 人",
                "操作",
            ]
            .into_iter()
            .zip(columns)
            {
                let text =
                    ui.painter()
                        .layout(label.into(), egui::FontId::proportional(16.0), MUTED, w);
                ui.painter().galley(
                    egui::pos2(x, header.center().y - text.size().y / 2.0),
                    text,
                    MUTED,
                );
                x += w + 20.0;
            }
            for item in items {
                let labels = [
                    item.team().to_owned(),
                    item.score_for(1).map(|s| fmt(s[0])).unwrap_or_default(),
                    item.score_for(2).map(|s| fmt(s[0])).unwrap_or_default(),
                    item.score_for(3).map(|s| fmt(s[0])).unwrap_or_default(),
                    item.score_for(3)
                        .map(|s| format!("{} / 160", s[7]))
                        .unwrap_or_default(),
                ];
                let text: Vec<_> = labels
                    .into_iter()
                    .zip(columns)
                    .map(|(label, w)| {
                        ui.painter()
                            .layout(label, egui::FontId::proportional(16.0), INK, w)
                    })
                    .collect();
                let height = text
                    .iter()
                    .map(|t| t.size().y + 24.0)
                    .fold(64.0_f32, f32::max);
                let (row, _) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                ui.painter().rect_filled(row, CARD_RADIUS, SURFACE);
                ui.painter().rect_stroke(
                    row,
                    CARD_RADIUS,
                    egui::Stroke::new(1.0_f32, BORDER),
                    egui::StrokeKind::Inside,
                );
                let mut x = row.left() + 12.0;
                for (text, w) in text.into_iter().zip(columns) {
                    let y = row.center().y - text.size().y / 2.0;
                    ui.painter().galley(egui::pos2(x, y), text, INK);
                    x += w + 20.0;
                }
                self.record_actions(
                    ui,
                    &item,
                    egui::Rect::from_min_size(
                        egui::pos2(x, row.center().y - 20.0),
                        egui::vec2(296.0, 40.0),
                    ),
                );
            }
        } else {
            for item in items {
                Self::section(ui, |ui| {
                    ui.label(RichText::new(item.team()).size(18.0).strong());
                    ui.columns(2, |columns| {
                        for (q, label) in [
                            (1, "第一问 / 分钟"),
                            (2, "第二问 / 分钟"),
                            (3, "第三问 / 分钟"),
                        ] {
                            let column = &mut columns[(q - 1) % 2];
                            column.label(RichText::new(label).size(13.0).color(MUTED));
                            column.label(item.score_for(q).map(|s| fmt(s[0])).unwrap_or_default());
                        }
                        columns[1].label(RichText::new("临时任务 / 人").size(13.0).color(MUTED));
                        columns[1].label(
                            item.score_for(3)
                                .map(|s| format!("{} / 160", s[7]))
                                .unwrap_or_default(),
                        );
                    });
                    let width = ui.available_width();
                    let (row, _) =
                        ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
                    let actions = egui::Rect::from_min_size(
                        egui::pos2(row.right() - width.min(296.0), row.top()),
                        egui::vec2(width.min(296.0), 40.0),
                    );
                    self.record_actions(ui, &item, actions);
                });
            }
        }
    }
    pub(super) fn record_actions(&mut self, ui: &mut egui::Ui, item: &Verified, area: egui::Rect) {
        let gap = if area.width() >= 296.0 { 12.0 } else { 8.0 };
        let widths = if area.width() >= 296.0 {
            [80.0, 80.0, 112.0]
        } else {
            let unit = (area.width() - 2.0 * gap) / 4.0;
            [unit, unit, unit * 2.0]
        };
        let mut x = area.left();
        for (index, (label, width)) in ["查看", "更新", "导出证书"]
            .into_iter()
            .zip(widths)
            .enumerate()
        {
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, area.top()), egui::vec2(width, 40.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
            ));
            child.spacing_mut().button_padding = egui::vec2(8.0, 8.0);
            let clicked = child
                .add_enabled(
                    index != 1 || !self.busy(),
                    egui::Button::new(RichText::new(label).size(14.0)).min_size(rect.size()),
                )
                .clicked();
            if clicked {
                match index {
                    0 => {
                        self.set_review(item.clone(), None, ui.ctx());
                        self.page = Page::Verify;
                        self.reset_scroll = true;
                    }
                    1 => {
                        if let Err(e) = self.use_certificate(item) {
                            self.show_error(e);
                        }
                    }
                    _ => self.export(item, ui.ctx()),
                }
            }
            x += width + gap;
        }
    }
    fn navigation_buttons(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.spacing_mut().button_padding = egui::vec2(if width < 80.0 { 4.0 } else { 8.0 }, 8.0);
        for (page, label) in Page::NAVIGATION {
            if ui
                .add_sized(
                    [width, 44.0],
                    egui::Button::selectable(
                        self.page == page,
                        RichText::new(label).size(if width < 80.0 { 14.0 } else { 16.0 }),
                    ),
                )
                .clicked()
            {
                self.page = page;
            }
        }
    }
    pub(super) fn navigation(&mut self, ui: &mut egui::Ui) {
        const TITLE: &str = "海上油田人员直升机运载计划编排";
        let title = || RichText::new(TITLE).color(ACCENT).size(21.0).strong();
        let title_width = ui
            .painter()
            .layout_no_wrap(TITLE.into(), egui::FontId::proportional(21.0), ACCENT)
            .size()
            .x;
        let width = ui.available_width();
        ui.spacing_mut().item_spacing.x = 8.0;
        if width >= title_width + 24.0 + 4.0 * 96.0 + 3.0 * 8.0 + 24.0 + 40.0 {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 44.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(title());
                    ui.add_space(16.0);
                    self.navigation_buttons(ui, 96.0);
                    ui.add_space((ui.available_width() - 40.0).max(0.0));
                    self.storage_button(ui);
                },
            );
        } else {
            // Keep all four destinations on one line, with settings beside the title.
            ui.allocate_ui_with_layout(
                egui::vec2(width, 44.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2((width - 48.0).max(0.0), 44.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(egui::Label::new(title()).wrap());
                        },
                    );
                    self.storage_button(ui);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(width, 44.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let gap = if width < 360.0 { 4.0 } else { 8.0 };
                    ui.spacing_mut().item_spacing.x = gap;
                    self.navigation_buttons(ui, (width - 3.0 * gap) / 4.0);
                },
            );
        }
    }
}
