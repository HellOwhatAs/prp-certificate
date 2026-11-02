use crate::certificate::{self, Imported, Verified, fmt};
use crate::platform::{self, Backend, FileEvent, FileQueue};
use base64::{Engine, engine::general_purpose::STANDARD};
use eframe::egui::{self, Color32, RichText};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const ACCENT: Color32 = Color32::from_rgb(40, 83, 66);
const INK: Color32 = Color32::from_rgb(37, 43, 39);
const MUTED: Color32 = Color32::from_rgb(113, 119, 113);
const RED: Color32 = Color32::from_rgb(166, 54, 47);
const NAMES: [&str; 3] = [
    "第一问 · 出海运输",
    "第二问 · 双向运输",
    "第三问 · 多日排班",
];
const SLOT_KEYS: [&str; 2] = ["routes", "assignments"];
const SAVE_KEY: &str = "prp-combined-certificates-v1";

#[derive(Default)]
struct CsvFile {
    name: String,
    text: String,
}
#[derive(Default)]
struct Question {
    files: [CsvFile; 2],
    checked: Option<Vec<u64>>,
    proof: Option<Verified>,
    error: String,
}
impl Question {
    fn has_files(&self) -> bool {
        self.files.iter().any(|f| !f.name.is_empty())
    }
    fn complete_files(&self) -> bool {
        self.files
            .iter()
            .all(|f| !f.name.is_empty() && !f.text.trim().is_empty())
    }
    fn proof_for(&self, team: &str) -> Option<&Verified> {
        self.proof.as_ref().filter(|p| p.team() == team.trim())
    }
    fn active(&self) -> bool {
        self.has_files() || self.proof.is_some()
    }
    fn changed(&mut self) {
        self.checked = None;
        self.proof = None;
        self.error.clear();
    }
}
fn combine_questions(questions: &[Question; 3], team: &str) -> Option<Verified> {
    let mut parts = vec![];
    for question in questions {
        if question.active() {
            parts.push(question.proof_for(team)?.clone());
        }
    }
    Verified::combine(&parts).ok()
}
#[derive(Clone, Copy)]
enum Kind {
    Check(usize),
    Prove(usize),
    Import,
}
struct Task {
    kind: Kind,
    import: Option<Pending>,
}
struct Pending {
    imported: Imported,
    name: String,
    stored: bool,
}
struct Batch {
    checks: VecDeque<usize>,
    proofs: VecDeque<usize>,
}
struct Verification {
    name: String,
    item: Option<Verified>,
    error: String,
}

pub struct FlightApp {
    page: usize,
    editing_record: Option<String>,
    reset_scroll: bool,
    team: String,
    questions: [Question; 3],
    backend: Backend,
    file_events: FileQueue,
    task: Option<Task>,
    batch: Option<Batch>,
    progress: String,
    status: String,
    error: bool,
    records: Vec<Verified>,
    pending: VecDeque<Pending>,
    verifications: Vec<Verification>,
    draft_texture: Option<egui::TextureHandle>,
    review_texture: Option<egui::TextureHandle>,
    review: Option<Verified>,
    ready: Option<Verified>,
    dirty: bool,
    confirm_clear: bool,
    imports_paused: bool,
}
impl FlightApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".into(),
            egui::FontData::from_static(certificate::FONT).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".into());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("cjk".into());
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = Color32::from_rgb(244, 245, 241);
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.text_edit_bg_color = Some(Color32::from_rgb(247, 248, 245));
        style.visuals.selection.bg_fill = Color32::from_rgb(224, 233, 225);
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(238, 241, 235);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(238, 241, 235);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(224, 233, 225);
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
        cc.egui_ctx.set_style_of(egui::Theme::Light, style);
        let saved: Vec<Value> = cc
            .storage
            .and_then(|s| eframe::get_value(s, SAVE_KEY))
            .unwrap_or_default();
        let pending = saved
            .into_iter()
            .take(200)
            .map(|certificate| Pending {
                imported: Imported {
                    certificate,
                    original: None,
                },
                name: "已保存证书".into(),
                stored: true,
            })
            .collect();
        Self {
            page: 0,
            editing_record: None,
            reset_scroll: false,
            team: String::new(),
            questions: Default::default(),
            backend: Backend::new(),
            file_events: Arc::new(Mutex::new(vec![])),
            task: None,
            batch: None,
            progress: String::new(),
            status: String::new(),
            error: false,
            records: vec![],
            pending,
            verifications: vec![],
            draft_texture: None,
            review_texture: None,
            review: None,
            ready: None,
            dirty: true,
            confirm_clear: false,
            imports_paused: false,
        }
    }
    fn busy(&self) -> bool {
        self.task.is_some() || self.batch.is_some()
    }
    fn show_error(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.error = true;
    }
    fn add_record(&mut self, item: Verified) {
        let id = item.id();
        if !self.records.iter().any(|r| r.id() == id) {
            self.records.insert(0, item);
            self.records.truncate(200);
        }
    }
    fn payload(&self, action: &str, q: usize) -> Value {
        let files: serde_json::Map<_, _> = self.questions[q]
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (SLOT_KEYS[i].into(), Value::String(f.text.clone())))
            .collect();
        json!({"action": action, "q": q+1, "team": self.team.trim(), "files": files})
    }
    fn send(&mut self, kind: Kind, payload: Value, import: Option<Pending>) -> bool {
        match self.backend.send(payload) {
            Ok(()) => {
                self.task = Some(Task { kind, import });
                self.progress = "处理中…".into();
                self.error = false;
                true
            }
            Err(e) => {
                self.batch = None;
                self.show_error(e);
                false
            }
        }
    }
    fn ready_certificate(&self) -> Option<Verified> {
        combine_questions(&self.questions, &self.team)
    }
    fn check_question(&mut self, q: usize) {
        if self.busy() {
            return;
        }
        self.questions[q].error.clear();
        self.questions[q].checked = None;
        if !self.questions[q].complete_files() {
            self.questions[q].error = "请选择完整的航线表和人员分配表".into();
            self.show_error(format!("第 {} 问：请选择完整的航线表和人员分配表", q + 1));
            return;
        }
        self.status.clear();
        self.send(Kind::Check(q), self.payload("check", q), None);
    }
    fn generate_certificate(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let team = self.team.trim();
        if team.is_empty() || team.chars().count() > 40 || team.chars().any(char::is_control) {
            self.show_error("请输入 1–40 字的队伍名称");
            return;
        }
        let mut checks = VecDeque::new();
        let mut proofs = VecDeque::new();
        let mut any = false;
        for q in 0..3 {
            let question = &mut self.questions[q];
            question.error.clear();
            if !question.active() {
                continue;
            }
            any = true;
            if question.has_files() && !question.complete_files() {
                question.error = "请选择完整的航线表和人员分配表".into();
                self.show_error(format!("第 {} 问：请选择完整的航线表和人员分配表", q + 1));
                return;
            }
            if question.proof_for(&self.team).is_none() && !question.complete_files() {
                question.error = "队伍名称与证书不一致，请重新选择方案文件".into();
                self.show_error(format!("第 {} 问：队伍名称与证书不一致", q + 1));
                return;
            }
            if question.complete_files() && question.proof_for(&self.team).is_none() {
                checks.push_back(q);
                proofs.push_back(q);
            }
        }
        if !any {
            self.show_error("请选择方案文件或更新证书记录");
            return;
        }
        self.status.clear();
        self.batch = Some(Batch { checks, proofs });
        self.next_batch(ctx);
    }
    fn next_batch(&mut self, ctx: &egui::Context) {
        if self.task.is_some() {
            return;
        }
        let Some(batch) = self.batch.as_mut() else {
            return;
        };
        if let Some(q) = batch.checks.pop_front() {
            self.send(Kind::Check(q), self.payload("check", q), None);
            return;
        }
        if let Some(q) = batch.proofs.pop_front() {
            self.send(Kind::Prove(q), self.payload("prove", q), None);
            return;
        }
        self.batch = None;
        if let Some(item) = self.ready_certificate() {
            if let Some(previous) = self.editing_record.take() {
                // A renamed draft creates a new record; it never rewrites the original team's certificate.
                self.records
                    .retain(|record| record.id() != previous || record.team() != item.team());
                self.editing_record = Some(item.id());
            }
            self.ready = Some(item.clone());
            self.add_record(item);
            self.status = "证书已生成".into();
            self.error = false;
        } else {
            self.show_error("证书未生成");
        }
        self.dirty = true;
        self.refresh_draft(ctx);
    }
    fn cancel(&mut self) {
        self.backend.cancel();
        if let Some(task) = self.task.take() {
            if let Some(import) = task.import {
                self.pending.push_front(import);
            }
        }
        self.batch = None;
        self.imports_paused = true;
        self.status = "已取消".into();
        self.error = false;
        self.progress.clear();
        self.dirty = true;
    }
    fn next_import(&mut self) {
        if self.busy() || self.imports_paused {
            return;
        }
        if let Some(p) = self.pending.pop_front() {
            let payload = if p.imported.certificate.is_null() {
                json!({"action":"import", "file": STANDARD.encode(p.imported.original.as_deref().unwrap_or_default())})
            } else {
                json!({"action":"verify", "certificate": p.imported.certificate})
            };
            self.send(Kind::Import, payload, Some(p));
        }
    }
    fn use_certificate(&mut self, item: &Verified) -> Result<(), String> {
        let parts = item.parts()?;
        self.team = item.team().to_owned();
        self.editing_record = Some(item.id());
        self.ready = Some(item.clone());
        self.questions = Default::default();
        for part in parts {
            let q = part.question() as usize - 1;
            self.questions[q].proof = Some(part);
        }
        self.dirty = true;
        self.reset_scroll = true;
        self.status.clear();
        self.error = false;
        self.page = 0;
        Ok(())
    }
    fn invalidate_generated(&mut self) {
        self.ready = None;
        self.dirty = true;
        self.status.clear();
    }
    fn texture(ctx: &egui::Context, key: &str, pixels: &[u8]) -> egui::TextureHandle {
        ctx.load_texture(
            key,
            egui::ColorImage::from_rgb(
                [certificate::WIDTH as usize, certificate::HEIGHT as usize],
                pixels,
            ),
            egui::TextureOptions::LINEAR,
        )
    }
    fn refresh_draft(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        let pixels = if let Some(item) = &self.ready {
            certificate::render(item).ok()
        } else {
            None
        };
        let pixels = pixels.unwrap_or_else(|| {
            let scores = std::array::from_fn(|q| {
                self.questions[q]
                    .proof_for(&self.team)
                    .map(|p| p.score().to_vec())
                    .or_else(|| self.questions[q].checked.clone())
            });
            certificate::render_preview(self.team.trim(), &scores)
        });
        self.draft_texture = Some(Self::texture(ctx, "draft", &pixels));
        self.dirty = false;
    }
    fn set_review(&mut self, item: Verified, ctx: &egui::Context) {
        match certificate::render(&item) {
            Ok(p) => {
                self.review_texture = Some(Self::texture(ctx, "review", &p));
                self.review = Some(item);
            }
            Err(e) => self.show_error(e),
        }
    }
    fn receive(&mut self, ctx: &egui::Context) {
        let files = std::mem::take(&mut *self.file_events.lock().unwrap());
        for event in files {
            match event {
                FileEvent::Loaded { slot, name, bytes } if slot == 6 => {
                    if bytes.len() > 16_000_000 {
                        self.show_error("证书图片不能超过 16 MB");
                    } else {
                        self.pending.push_back(Pending {
                            imported: Imported {
                                certificate: Value::Null,
                                original: Some(bytes),
                            },
                            name,
                            stored: false,
                        });
                        self.imports_paused = false;
                    }
                }
                FileEvent::Loaded { slot, name, bytes } if slot < 6 && !self.busy() => {
                    if bytes.len() > 10_000_000 {
                        self.show_error("单个 CSV 不能超过 10 MB");
                        continue;
                    }
                    match String::from_utf8(bytes) {
                        Ok(text) => {
                            let question = &mut self.questions[slot / 2];
                            question.changed();
                            question.files[slot % 2] = CsvFile { name, text };
                            if question.files[slot % 2].text.trim().is_empty() {
                                question.error = "CSV 为空".into();
                            }
                            self.invalidate_generated();
                        }
                        Err(_) => self.show_error("请使用 UTF-8 编码的 CSV"),
                    }
                }
                FileEvent::Loaded { .. } => self.show_error("请在当前任务结束后选择文件"),
                FileEvent::Saved(name) => {
                    self.status = format!("已导出 {name}");
                    self.error = false;
                }
                FileEvent::Error(e) => self.show_error(e),
            }
        }
        for message in self.backend.drain() {
            match message["type"].as_str() {
                Some("progress") => {
                    let text = message["text"]
                        .as_str()
                        .unwrap_or("")
                        .replace("加载 Rust 计算引擎", "加载计算组件")
                        .replace("读取固定公开参数", "准备证明组件")
                        .replace("读取公开参数", "准备证明组件")
                        .replace("构建 Groth16 约束", "检查证明数据")
                        .replace("生成 Groth16 零知识证明", "生成证书")
                        .replace("计算证明多项式", "生成证书")
                        .replace("生成证明", "生成证书")
                        .replace("验证 Groth16 成绩证明", "验证证书");
                    self.progress = match self.task.as_ref().map(|t| t.kind) {
                        Some(Kind::Check(q) | Kind::Prove(q)) => format!("第 {} 问：{text}", q + 1),
                        _ => text,
                    };
                }
                Some("error") => {
                    let error = message["message"].as_str().unwrap_or("计算失败").to_owned();
                    if let Some(task) = self.task.take() {
                        match task.kind {
                            Kind::Check(q) | Kind::Prove(q) => {
                                self.questions[q].error = error.clone();
                                self.batch = None;
                                self.show_error(format!("第 {} 问：{error}", q + 1));
                                self.dirty = true;
                            }
                            Kind::Import => {
                                if let Some(p) = task.import {
                                    self.verifications.insert(
                                        0,
                                        Verification {
                                            name: p.name,
                                            item: None,
                                            error: error.clone(),
                                        },
                                    );
                                }
                                self.show_error(error);
                            }
                        }
                    }
                    self.progress.clear();
                }
                Some("result") => {
                    let Some(task) = self.task.take() else {
                        continue;
                    };
                    let result = &message["result"];
                    match task.kind {
                        Kind::Check(q) => {
                            let standalone = self.batch.is_none();
                            let scores: Result<Vec<Vec<u64>>, _> =
                                serde_json::from_value(result["scores"].clone());
                            match scores
                                .ok()
                                .and_then(|s| s.into_iter().next())
                                .filter(|s| s.len() == 9 && s[6] > 0)
                            {
                                Some(scores) => {
                                    self.questions[q].checked = Some(scores);
                                    if standalone {
                                        self.status = format!("第 {} 问：校验通过", q + 1);
                                        self.error = false;
                                    }
                                    if result["canProve"] != true {
                                        self.questions[q].error =
                                            "超过 256 架次，无法生成证书".into();
                                        if self.batch.is_some() {
                                            self.batch = None;
                                            self.show_error(format!(
                                                "第 {} 问：超过 256 架次，无法生成证书",
                                                q + 1
                                            ));
                                        }
                                    }
                                }
                                None => {
                                    self.batch = None;
                                    self.show_error("校验结果无效");
                                }
                            }
                            self.dirty = true;
                        }
                        Kind::Prove(q) => {
                            match serde_json::from_value::<Verified>(result.clone())
                                .map_err(|e| e.to_string())
                                .and_then(|v| {
                                    v.set()?;
                                    Ok(v)
                                }) {
                                Ok(item)
                                    if item.question() == (q + 1) as u64
                                        && item.team() == self.team.trim() =>
                                {
                                    self.questions[q].proof = Some(item);
                                    self.questions[q].error.clear();
                                    self.dirty = true;
                                }
                                Ok(_) => {
                                    self.batch = None;
                                    self.show_error("证明结果与当前方案不一致");
                                }
                                Err(e) => {
                                    self.batch = None;
                                    self.show_error(e);
                                }
                            }
                        }
                        Kind::Import => {
                            let p = task.import.unwrap();
                            let verified = serde_json::from_value::<Verified>(result.clone())
                                .map_err(|e| e.to_string())
                                .and_then(|v| {
                                    v.set()?;
                                    if !p.imported.certificate.is_null() {
                                        certificate::check_document(&p.imported, &v)?;
                                    }
                                    Verified::combine(&[v])
                                });
                            match verified {
                                Ok(item) => {
                                    self.add_record(item.clone());
                                    if !p.stored {
                                        self.set_review(item.clone(), ctx);
                                        self.verifications.insert(
                                            0,
                                            Verification {
                                                name: p.name,
                                                item: Some(item),
                                                error: String::new(),
                                            },
                                        );
                                        self.status = "验证通过".into();
                                        self.error = false;
                                    }
                                }
                                Err(e) => {
                                    self.verifications.insert(
                                        0,
                                        Verification {
                                            name: p.name,
                                            item: None,
                                            error: e.clone(),
                                        },
                                    );
                                    self.show_error(e);
                                }
                            }
                        }
                    }
                    self.progress.clear();
                    self.next_batch(ctx);
                }
                _ => {}
            }
        }
        self.verifications.truncate(20);
        self.next_import();
        self.refresh_draft(ctx);
    }
    fn export(&mut self, item: &Verified, ctx: &egui::Context) {
        let team: String = item
            .team()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(40)
            .collect();
        let name = format!("PRP-{}-{}.png", team, &item.id()[..8]);
        match certificate::export_png(item) {
            Ok(bytes) => platform::save(name, bytes, self.file_events.clone(), ctx.clone()),
            Err(e) => self.show_error(e),
        }
    }
    fn section(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(222, 226, 218)))
            .corner_radius(8)
            .inner_margin(20)
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                body(ui);
            });
    }
    fn template(&mut self, q: usize, assignment: bool, ctx: &egui::Context) {
        let table = match (q, assignment) {
            (0, false) => include_bytes!("../../../public/templates/q1-routes.csv").as_slice(),
            (0, true) => include_bytes!("../../../public/templates/q1-assignments.csv").as_slice(),
            (1, false) => include_bytes!("../../../public/templates/q2-routes.csv").as_slice(),
            (1, true) => include_bytes!("../../../public/templates/q2-assignments.csv").as_slice(),
            (2, false) => include_bytes!("../../../public/templates/q3-routes.csv").as_slice(),
            _ => include_bytes!("../../../public/templates/q3-assignments.csv").as_slice(),
        };
        platform::save(
            format!(
                "q{}-{}.csv",
                q + 1,
                if assignment { "assignments" } else { "routes" }
            ),
            table.to_vec(),
            self.file_events.clone(),
            ctx.clone(),
        );
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
                let can_clear = question.active() && !self.busy();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            can_clear,
                            egui::Button::new(RichText::new("清空").size(14.0).color(MUTED))
                                .frame(false)
                                .small(),
                        )
                        .clicked()
                    {
                        self.questions[q] = Question::default();
                        self.invalidate_generated();
                    }
                });
            });
            if ui.available_width() >= 400.0 {
                ui.columns(2, |columns| {
                    self.file_button(&mut columns[0], q, 0, "航线表");
                    self.file_button(&mut columns[1], q, 1, "人员分配表");
                });
            } else {
                self.file_button(ui, q, 0, "航线表");
                self.file_button(ui, q, 1, "人员分配表");
            }
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("航线模板").size(13.0).color(MUTED))
                            .small()
                            .frame(false),
                    )
                    .clicked()
                {
                    self.template(q, false, ui.ctx());
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("人员分配模板").size(13.0).color(MUTED))
                            .small()
                            .frame(false),
                    )
                    .clicked()
                {
                    self.template(q, true, ui.ctx());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            self.questions[q].has_files() && !self.busy(),
                            egui::Button::new("仅校验").min_size(egui::vec2(84.0, 34.0)),
                        )
                        .clicked()
                    {
                        self.check_question(q);
                    }
                });
            });
            if !self.questions[q].error.is_empty() {
                ui.colored_label(RED, &self.questions[q].error);
            }
        });
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
                    .fill(Color32::from_rgb(247, 248, 245))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(215, 222, 212)))
                    .corner_radius(5)
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
            ui.label("队伍名称");
            if ui
                .add_enabled(
                    !self.busy(),
                    egui::TextEdit::singleline(&mut self.team)
                        .id_salt("certificate-team-name")
                        .hint_text(RichText::new("1–40 字").color(MUTED))
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
            let generate = ui
                .add_enabled_ui(
                    !self.busy() && self.questions.iter().any(Question::active),
                    |ui| {
                        ui.add_sized(
                            size,
                            egui::Button::new(RichText::new("生成证书").color(Color32::WHITE))
                                .fill(ACCENT)
                                .corner_radius(6),
                        )
                        .clicked()
                    },
                )
                .inner;
            if generate {
                self.generate_certificate(ui.ctx());
            }
            let export = ui
                .add_enabled_ui(self.ready.is_some() && !self.busy(), |ui| {
                    ui.add_sized(size, egui::Button::new("导出证书").corner_radius(6))
                        .clicked()
                })
                .inner;
            if export {
                if let Some(item) = self.ready.clone() {
                    self.export(&item, ui.ctx());
                }
            }
        });
    }
    fn picture(ui: &mut egui::Ui, texture: &Option<egui::TextureHandle>) {
        if let Some(texture) = texture {
            let width = ui.available_width();
            let height = width * certificate::HEIGHT as f32 / certificate::WIDTH as f32;
            ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(width, height)));
        }
    }
    fn save_button(&mut self, ui: &mut egui::Ui, item: Option<Verified>) {
        let clicked = ui
            .add_enabled_ui(item.is_some() && !self.busy(), |ui| {
                ui.add_sized(
                    [ui.available_width(), 46.0],
                    egui::Button::new(RichText::new("导出证书").color(Color32::WHITE))
                        .fill(ACCENT)
                        .corner_radius(6),
                )
                .clicked()
            })
            .inner;
        if clicked {
            if let Some(item) = item {
                self.export(&item, ui.ctx());
            }
        }
    }
    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        Self::picture(ui, &self.draft_texture);
    }
    fn generate_page(&mut self, ui: &mut egui::Ui) {
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
    fn verification_list(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        for (i, v) in self.verifications.iter().enumerate() {
            Self::section(ui, |ui| {
                ui.label(RichText::new(&v.name).strong());
                if let Some(item) = &v.item {
                    ui.label(item.team());
                    for q in 1..=3 {
                        ui.horizontal(|ui| {
                            ui.label(format!("第 {q} 问"));
                            if let Some(s) = item.score_for(q) {
                                ui.colored_label(ACCENT, "验证通过");
                                ui.label(format!("{} 分钟", fmt(s[0])));
                            } else {
                                ui.label(RichText::new("未提供").color(MUTED));
                            }
                        });
                    }
                    if ui.small_button("查看").clicked() {
                        selected = Some(i);
                    }
                } else {
                    ui.colored_label(RED, "验证失败");
                    ui.label(&v.error);
                }
            });
        }
        if let Some(i) = selected {
            self.set_review(self.verifications[i].item.clone().unwrap(), ui.ctx());
        }
    }
    fn review_panel(&mut self, ui: &mut egui::Ui) {
        if self.review.is_some() {
            self.save_button(ui, self.review.clone());
            ui.add_space(2.0);
            Self::picture(ui, &self.review_texture);
        }
    }
    fn verify_page(&mut self, ui: &mut egui::Ui) {
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
            ui.columns(2, |cols| {
                self.verification_list(&mut cols[0]);
                self.review_panel(&mut cols[1]);
            });
        } else {
            self.verification_list(ui);
            self.review_panel(ui);
        }
    }
    fn board_page(&mut self, ui: &mut egui::Ui) {
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
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("records")
                .spacing([24.0, 18.0])
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    for label in [
                        "队伍",
                        "第一问 / 分钟",
                        "第二问 / 分钟",
                        "第三问 / 分钟",
                        "临时任务 / 人",
                        "操作",
                    ] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for item in items {
                        ui.allocate_ui_with_layout(
                            egui::vec2(200.0, 34.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(item.team());
                            },
                        );
                        for q in 1..=3 {
                            ui.label(item.score_for(q).map(|s| fmt(s[0])).unwrap_or_default());
                        }
                        ui.label(
                            item.score_for(3)
                                .map(|s| format!("{} / 160", s[7]))
                                .unwrap_or_default(),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized([64.0, 34.0], egui::Button::new("查看"))
                                .clicked()
                            {
                                self.set_review(item.clone(), ui.ctx());
                                self.page = 1;
                            }
                            if ui
                                .add_enabled_ui(!self.busy(), |ui| {
                                    ui.add_sized([64.0, 34.0], egui::Button::new("更新"))
                                        .clicked()
                                })
                                .inner
                            {
                                if let Err(e) = self.use_certificate(&item) {
                                    self.show_error(e);
                                }
                            }
                            if ui
                                .add_sized([100.0, 34.0], egui::Button::new("导出证书"))
                                .clicked()
                            {
                                self.export(&item, ui.ctx());
                            }
                        });
                        ui.end_row();
                    }
                });
        });
    }
}
impl eframe::App for FlightApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.busy() {
            100
        } else {
            350
        }));
        egui::TopBottomPanel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(24, 18)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("海上油田人员直升机运载计划编排")
                            .size(21.0)
                            .strong(),
                    );
                    ui.add_space(20.0);
                    for (i, label) in ["生成证书", "验证证书", "证书记录"].iter().enumerate()
                    {
                        ui.selectable_value(&mut self.page, i, *label);
                    }
                });
            });
        if self.busy() || !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status")
                .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(14))
                .show(ctx, |ui| {
                    if self.busy() {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label(&self.progress);
                            if ui.button("取消").clicked() {
                                self.cancel();
                            }
                        });
                    } else {
                        ui.colored_label(if self.error { RED } else { ACCENT }, &self.status);
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(244, 245, 241))
                    .inner_margin(24),
            )
            .show(ctx, |ui| {
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("page-scroll", self.page))
                    .auto_shrink([false, false]);
                if std::mem::take(&mut self.reset_scroll) {
                    scroll = scroll.vertical_scroll_offset(0.0);
                }
                scroll.show(ui, |ui| match self.page {
                    0 => self.generate_page(ui),
                    1 => self.verify_page(ui),
                    _ => self.board_page(ui),
                });
            });
        if self.confirm_clear {
            egui::Window::new("清空记录")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("清空本机证书记录？");
                    ui.horizontal(|ui| {
                        if ui.button("清空").clicked() {
                            self.records.clear();
                            self.pending.clear();
                            self.verifications.clear();
                            self.review = None;
                            self.review_texture = None;
                            self.confirm_clear = false;
                        }
                        if ui.button("取消").clicked() {
                            self.confirm_clear = false;
                        }
                    });
                });
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let mut certs: Vec<Value> = self.records.iter().map(|r| r.certificate.clone()).collect();
        certs.extend(
            self.pending
                .iter()
                .filter(|p| p.stored)
                .map(|p| p.imported.certificate.clone()),
        );
        if let Some(p) = self
            .task
            .as_ref()
            .and_then(|t| t.import.as_ref())
            .filter(|p| p.stored)
        {
            certs.push(p.imported.certificate.clone());
        }
        eframe::set_value(storage, SAVE_KEY, &certs);
    }
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn proof(q: usize) -> Verified {
        let c = flight_format::Certificate {
            metadata: flight_format::Metadata::new(
                q,
                "甲".into(),
                flight_format::nonce(&[q as u8; 16]),
            ),
            proof: STANDARD.encode([42; 128]),
            scores: [1; 9],
        };
        Verified {
            certificate: serde_json::to_value(flight_format::CertificateSet::new(vec![c]).unwrap())
                .unwrap(),
            scores: vec![vec![1; 9]],
        }
    }
    #[test]
    fn incomplete_question_cannot_silently_disappear_from_export() {
        let mut questions: [Question; 3] = Default::default();
        questions[0].proof = Some(proof(1));
        questions[2].proof = Some(proof(3));
        let item = combine_questions(&questions, "甲").unwrap();
        assert!(item.score_for(1).is_some());
        assert!(item.score_for(2).is_none());
        assert!(item.score_for(3).is_some());
        questions[1].files[0] = CsvFile {
            name: "routes.csv".into(),
            text: "unfinished".into(),
        };
        assert!(combine_questions(&questions, "甲").is_none());
        // An explicit removal allows export of the two retained questions.
        questions[1] = Question::default();
        assert!(combine_questions(&questions, "甲").is_some());
        assert!(combine_questions(&questions, "乙").is_none());
        questions[0].changed();
        questions[0].files[0] = CsvFile {
            name: "new.csv".into(),
            text: "new".into(),
        };
        assert!(questions[2].proof.is_some());
        assert!(combine_questions(&questions, "甲").is_none());
        assert!(combine_questions(&Default::default(), "甲").is_none());
    }
}
