use crate::certificate::{self, Imported, Verified, fmt};
use crate::platform::{self, Backend, FileEvent, FileQueue};
use base64::{Engine, engine::general_purpose::STANDARD};
use eframe::egui::{self, Color32, RichText};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
const INK: Color32 = Color32::from_rgb(29, 54, 59);
const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
const NAMES: [&str; 4] = ["", "出海运输", "双向运输", "多日排班"];
const SLOT_KEYS: [&str; 2] = ["routes", "assignments"];
const SAVE_KEY: &str = "prp-public-certificates-v5";

#[derive(Default)]
struct CsvFile {
    name: String,
    text: String,
}
struct Task {
    action: String,
    imported: Option<Imported>,
    name: String,
}
struct Verification {
    title: String,
    ok: bool,
    detail: String,
}

pub struct FlightApp {
    page: usize,
    q: usize,
    board_q: usize,
    team: String,
    files: [CsvFile; 2],
    backend: Backend,
    file_events: FileQueue,
    task: Option<Task>,
    progress: String,
    status: String,
    error: bool,
    checked: Option<Vec<Vec<u64>>>,
    can_prove: bool,
    generated: Option<Verified>,
    records: Vec<Verified>,
    pending: VecDeque<(Imported, String)>,
    verifications: Vec<Verification>,
    preview: Option<Verified>,
    texture: Option<egui::TextureHandle>,
    about: bool,
    confirm_clear: bool,
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
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = Color32::from_rgb(245, 248, 247);
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.selection.bg_fill = Color32::from_rgb(211, 233, 226);
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(234, 240, 237);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(234, 240, 237);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(213, 232, 224);
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
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        cc.egui_ctx.set_style(style);
        let saved: Vec<Value> = cc
            .storage
            .and_then(|s| eframe::get_value(s, SAVE_KEY))
            .unwrap_or_default();
        let pending = saved
            .into_iter()
            .take(200)
            .map(|certificate| {
                (
                    Imported {
                        certificate,
                        original: None,
                    },
                    "已保存证书".into(),
                )
            })
            .collect();
        Self {
            page: 0,
            q: 1,
            board_q: 1,
            team: String::new(),
            files: Default::default(),
            backend: Backend::new(),
            file_events: Arc::new(Mutex::new(vec![])),
            task: None,
            progress: String::new(),
            status: String::new(),
            error: false,
            checked: None,
            can_prove: false,
            generated: None,
            records: vec![],
            pending,
            verifications: vec![],
            preview: None,
            texture: None,
            about: false,
            confirm_clear: false,
        }
    }
    fn invalidate(&mut self) {
        self.checked = None;
        self.can_prove = false;
        self.generated = None;
        self.status.clear();
    }
    fn show_error(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.error = true;
    }
    fn payload(&self, action: &str) -> Value {
        let mut files = serde_json::Map::new();
        for (i, f) in self.files.iter().enumerate() {
            files.insert(SLOT_KEYS[i].into(), Value::String(f.text.clone()));
        }
        json!({"action":action,"q":self.q,"team":self.team.trim(),"files":files})
    }
    fn submit(&mut self, action: &str, payload: Value, imported: Option<Imported>, name: String) {
        if self.task.is_some() {
            return;
        }
        match self.backend.send(payload) {
            Ok(()) => {
                self.task = Some(Task {
                    action: action.into(),
                    imported,
                    name,
                });
                self.progress = "处理中…".into();
                self.status.clear();
                self.error = false;
            }
            Err(e) => self.show_error(e),
        }
    }
    fn cancel(&mut self) {
        self.backend.cancel();
        if let Some(task) = self.task.take() {
            if let Some(imported) = task.imported {
                self.pending.push_front((imported, task.name));
            }
        }
        self.status = "任务已取消".into();
        self.error = false;
        self.progress.clear();
    }
    fn add_record(&mut self, item: Verified) {
        if !self.records.iter().any(|r| r.nonce() == item.nonce()) {
            self.records.push(item);
        }
    }
    fn set_preview(&mut self, item: Verified, ctx: &egui::Context) {
        match certificate::render(&item) {
            Ok(p) => {
                self.texture = Some(ctx.load_texture(
                    "certificate",
                    egui::ColorImage::from_rgb(
                        [certificate::WIDTH as usize, certificate::HEIGHT as usize],
                        &p,
                    ),
                    egui::TextureOptions::LINEAR,
                ));
                self.preview = Some(item);
            }
            Err(e) => self.show_error(e),
        }
    }
    fn next_verification(&mut self) {
        if self.task.is_none() {
            if let Some((imported, name)) = self.pending.pop_front() {
                let (action, payload) = if imported.certificate.is_null() {
                    (
                        "import",
                        json!({"action":"import","file":STANDARD.encode(imported.original.as_ref().unwrap())}),
                    )
                } else {
                    (
                        "verify",
                        json!({"action":"verify","certificate":imported.certificate}),
                    )
                };
                self.submit(action, payload, Some(imported), name);
            }
        }
    }
    fn receive(&mut self, ctx: &egui::Context) {
        let files = std::mem::take(&mut *self.file_events.lock().unwrap());
        for event in files {
            match event {
                FileEvent::Loaded { slot, name, bytes } => {
                    if slot == 4 {
                        if bytes.len() > 16_000_000 {
                            self.show_error("证书文件不能超过 16 MB");
                        } else {
                            self.pending.push_back((
                                Imported {
                                    certificate: Value::Null,
                                    original: Some(bytes),
                                },
                                name,
                            ));
                        }
                    } else if self.task.is_none() {
                        if bytes.len() > 10_000_000 {
                            self.show_error("单个 CSV 不能超过 10 MB");
                            continue;
                        }
                        match String::from_utf8(bytes) {
                            Ok(text) => {
                                self.files[slot] = CsvFile { name, text };
                                self.invalidate();
                                if self.files[slot].text.trim().is_empty() {
                                    self.show_error(format!(
                                        "{}为空，请选择包含数据的 CSV",
                                        if slot == 0 {
                                            "航线表"
                                        } else {
                                            "人员分配表"
                                        }
                                    ));
                                }
                            }
                            Err(_) => self.show_error("请使用 UTF-8 编码的 CSV"),
                        }
                    }
                }
                FileEvent::Saved(name) => {
                    self.status = format!("已保存 {name}");
                    self.error = false;
                }
                FileEvent::Error(e) => self.show_error(e),
            }
        }
        for message in self.backend.drain() {
            match message["type"].as_str() {
                Some("progress") => {
                    self.progress = message["text"]
                        .as_str()
                        .unwrap_or("")
                        .replace("加载 Rust 计算引擎", "加载计算组件")
                        .replace("读取固定公开参数", "准备证明组件")
                        .replace("读取公开参数", "准备证明组件")
                        .replace("构建 Groth16 约束", "检查证明数据")
                        .replace("生成 Groth16 零知识证明", "生成证书")
                        .replace("计算证明多项式", "生成证书")
                        .replace("生成证明", "生成证书")
                        .replace("验证 Groth16 成绩证明", "核验证书");
                }
                Some("error") => {
                    let error = message["message"].as_str().unwrap_or("计算失败").to_owned();
                    if let Some(task) = self.task.take() {
                        if task.action == "verify" || task.action == "import" {
                            self.verifications.insert(
                                0,
                                Verification {
                                    title: task.name,
                                    ok: false,
                                    detail: error.clone(),
                                },
                            );
                        }
                    }
                    self.show_error(error);
                    self.progress.clear();
                    self.next_verification();
                }
                Some("result") => {
                    let Some(task) = self.task.take() else {
                        continue;
                    };
                    let result = &message["result"];
                    if task.action == "check" {
                        self.checked = serde_json::from_value(result["scores"].clone()).ok();
                        self.can_prove = result["canProve"] == true;
                        self.status = if self.can_prove {
                            ""
                        } else {
                            "方案校验通过；超过 256 架次，暂不能生成证书"
                        }
                        .into();
                        self.error = false;
                    } else {
                        match serde_json::from_value::<Verified>(result.clone()) {
                            Ok(item) => {
                                let visual = task
                                    .imported
                                    .as_ref()
                                    .filter(|_| task.action != "import")
                                    .map(|p| certificate::check_document(p, &item))
                                    .unwrap_or(Ok(()));
                                match visual {
                                    Ok(()) => {
                                        let detail = format!(
                                            "问题 {} · {} 分钟 · {} 架次{}\n燃油 {}.{:01} kg · 在途 {} 分钟 · 座位利用率 {:.2}%",
                                            item.question(),
                                            fmt(item.score()[0]),
                                            item.score()[3],
                                            if item.question() == 3 {
                                                format!(" · 临时任务 {} / 160 人", item.score()[7])
                                            } else {
                                                String::new()
                                            },
                                            fmt(item.score()[2] / 10),
                                            item.score()[2] % 10,
                                            fmt(item.score()[1]),
                                            item.score()[5] as f64 / item.score()[6] as f64 * 100.0
                                        );
                                        self.status.clear();
                                        self.error = false;
                                        if task.action == "prove" {
                                            self.generated = Some(item.clone());
                                            self.set_preview(item.clone(), ctx);
                                        } else if task.action == "import" {
                                            self.verifications.insert(
                                                0,
                                                Verification {
                                                    title: format!(
                                                        "{} · {}",
                                                        task.name,
                                                        item.team()
                                                    ),
                                                    ok: true,
                                                    detail,
                                                },
                                            );
                                        }
                                        self.add_record(item);
                                    }
                                    Err(e) => {
                                        self.verifications.insert(
                                            0,
                                            Verification {
                                                title: task.name,
                                                ok: false,
                                                detail: e.clone(),
                                            },
                                        );
                                        self.show_error(e);
                                    }
                                }
                            }
                            Err(e) => self.show_error(format!("引擎返回格式无效：{e}")),
                        }
                    }
                    self.progress.clear();
                    self.next_verification();
                }
                _ => {}
            }
        }
        // Start stored-certificate verification only at startup / after explicit imports.
        if self.task.is_none() && !self.pending.is_empty() && self.status != "任务已取消" {
            self.next_verification();
        }
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
        let ext = "png";
        let name = format!(
            "PRP-Q{}-{}-{}.{}",
            item.question(),
            team,
            item.nonce().get(..8).unwrap_or(""),
            ext
        );
        let bytes = certificate::export_png(item);
        match bytes {
            Ok(bytes) => platform::save(name, bytes, self.file_events.clone(), ctx.clone()),
            Err(e) => self.show_error(e),
        }
    }
    fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(219, 228, 224)))
            .corner_radius(9)
            .inner_margin(22)
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                ui.label(RichText::new(title).size(20.0).strong());
                ui.add_space(10.0);
                body(ui);
            });
    }
    fn file_button(&mut self, ui: &mut egui::Ui, slot: usize, label: &str) {
        let name = if self.files[slot].name.is_empty() {
            "选择 CSV"
        } else {
            &self.files[slot].name
        };
        ui.label(RichText::new(label).color(MUTED));
        if ui
            .add_enabled(
                self.task.is_none(),
                egui::Button::new(name).min_size(egui::vec2(ui.available_width(), 42.0)),
            )
            .clicked()
        {
            platform::pick(slot, self.file_events.clone(), ui.ctx().clone());
        }
    }
    fn generate_form(&mut self, ui: &mut egui::Ui) {
        Self::section(ui, "方案", |ui| {
            ui.label("队伍名称");
            let edit = ui.add_enabled(
                self.task.is_none(),
                egui::TextEdit::singleline(&mut self.team)
                    .hint_text("1–40 字")
                    .desired_width(f32::INFINITY),
            );
            if edit.changed() {
                self.generated = None;
            }
            self.file_button(ui, 0, "航线表");
            self.file_button(ui, 1, "人员分配表");
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("航线模板").clicked() {
                    self.template(false, ui.ctx());
                }
                if ui.small_button("人员分配模板").clicked() {
                    self.template(true, ui.ctx());
                }
            });
            ui.add_space(6.0);
            let enabled =
                self.task.is_none() && self.files.iter().all(|f| !f.text.trim().is_empty());
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    let size = egui::vec2(
                        (ui.available_width() - ui.spacing().item_spacing.x) / 2.0,
                        44.0,
                    );
                    if ui
                        .add_sized(
                            size,
                            egui::Button::new(RichText::new("生成证书").color(Color32::WHITE))
                                .fill(ACCENT)
                                .corner_radius(6),
                        )
                        .clicked()
                    {
                        let team = self.team.trim();
                        if team.is_empty()
                            || team.chars().count() > 40
                            || team.chars().any(|c| c.is_control())
                        {
                            self.show_error("请输入 1–40 字的队伍名称");
                        } else {
                            self.generated = None;
                            self.submit("prove", self.payload("prove"), None, String::new());
                        }
                    }
                    if ui
                        .add_sized(size, egui::Button::new("仅校验").corner_radius(6))
                        .clicked()
                    {
                        self.generated = None;
                        self.checked = None;
                        self.can_prove = false;
                        self.submit("check", self.payload("check"), None, String::new());
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                if let Some(item) = self.generated.clone() {
                    if ui.button("保存证书 PNG").clicked() {
                        self.export(&item, ui.ctx());
                    }
                }
            });
        });
    }
    fn score_panel(&mut self, ui: &mut egui::Ui) {
        Self::section(ui, "成绩", |ui| {
            if let Some(item) = self.generated.as_ref() {
                ui.colored_label(ACCENT, "证书已生成");
                Self::metrics(ui, &item.scores, self.q);
            } else if let Some(scores) = self.checked.as_ref() {
                ui.label(
                    RichText::new("方案可行 · 未生成证书")
                        .size(14.0)
                        .color(MUTED),
                );
                Self::metrics(ui, scores, self.q);
            } else {
                ui.label(RichText::new("选择方案后可生成证书或仅校验").color(MUTED));
            }
        });
        if self.generated.as_ref().is_some_and(|g| {
            self.preview
                .as_ref()
                .is_some_and(|p| p.nonce() == g.nonce())
        }) {
            if let Some(texture) = &self.texture {
                ui.add_space(16.0);
                ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(
                    ui.available_width(),
                    ui.available_width() * certificate::HEIGHT as f32 / certificate::WIDTH as f32,
                )));
            }
        }
    }
    fn metrics(ui: &mut egui::Ui, scores: &[Vec<u64>], q: usize) {
        let Some(s) = scores.last().filter(|s| s.len() == 9 && s[6] > 0) else {
            return;
        };
        ui.add_space(10.0);
        ui.label(RichText::new("总飞机使用时间 / 分钟").color(MUTED));
        ui.label(RichText::new(fmt(s[0])).size(46.0));
        ui.add_space(8.0);
        ui.separator();
        let rows = [
            ("总架次数", fmt(s[3])),
            (
                "座位利用率",
                format!("{:.2}%", s[5] as f64 / s[6] as f64 * 100.0),
            ),
            (
                "总燃油消耗",
                format!("{}.{:01} kg", fmt(s[2] / 10), s[2] % 10),
            ),
            ("人员总在途时间", format!("{} 分钟", fmt(s[1]))),
        ];
        for (label, value) in rows {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.strong(value);
                });
            });
        }
        if q == 3 {
            ui.separator();
            ui.label(format!("临时任务  {} / 160 人", s[7]));
        }
    }
    fn template(&mut self, assignment: bool, ctx: &egui::Context) {
        let table = match (self.q, assignment) {
            (1, false) => include_bytes!("../../../public/templates/q1-routes.csv").as_slice(),
            (1, true) => include_bytes!("../../../public/templates/q1-assignments.csv").as_slice(),
            (2, false) => include_bytes!("../../../public/templates/q2-routes.csv").as_slice(),
            (2, true) => include_bytes!("../../../public/templates/q2-assignments.csv").as_slice(),
            (3, false) => include_bytes!("../../../public/templates/q3-routes.csv").as_slice(),
            _ => include_bytes!("../../../public/templates/q3-assignments.csv").as_slice(),
        };
        platform::save(
            format!(
                "q{}-{}.csv",
                self.q,
                if assignment { "assignments" } else { "routes" }
            ),
            table.to_vec(),
            self.file_events.clone(),
            ctx.clone(),
        );
    }
    fn generate_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for q in 1..=3 {
                let text = format!("问题 {q} · {}", NAMES[q]);
                if ui
                    .add_enabled(
                        self.task.is_none(),
                        egui::Button::selectable(self.q == q, text),
                    )
                    .clicked()
                {
                    self.q = q;
                    self.files = Default::default();
                    self.invalidate();
                }
            }
        });
        ui.add_space(14.0);
        if ui.available_width() > 800.0 {
            ui.columns(2, |cols| {
                self.generate_form(&mut cols[0]);
                self.score_panel(&mut cols[1]);
            });
        } else {
            self.generate_form(ui);
            ui.add_space(16.0);
            self.score_panel(ui);
        }
    }
    fn verify_page(&mut self, ui: &mut egui::Ui) {
        Self::section(ui, "证书图片", |ui| {
            ui.label(RichText::new("支持 PNG / JPEG，以验证后显示的成绩为准。").color(MUTED));
            if ui
                .add_enabled(
                    self.task.is_none(),
                    egui::Button::new("导入证书图片").min_size(egui::vec2(240.0, 48.0)),
                )
                .clicked()
            {
                self.status.clear();
                platform::pick(4, self.file_events.clone(), ui.ctx().clone());
            }
        });
        if !self.pending.is_empty() && self.task.is_none() {
            if ui
                .button(format!("继续验证 {} 份证书", self.pending.len()))
                .clicked()
            {
                self.status.clear();
                self.next_verification();
            }
        }
        ui.add_space(14.0);
        for v in self.verifications.iter().take(20) {
            egui::Frame::new()
                .fill(Color32::WHITE)
                .inner_margin(18)
                .corner_radius(8)
                .show(ui, |ui| {
                    ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                    ui.label(
                        RichText::new(format!(
                            "{}  {}",
                            if v.ok {
                                "✓ 验证通过"
                            } else {
                                "× 验证失败"
                            },
                            v.title
                        ))
                        .color(if v.ok {
                            ACCENT
                        } else {
                            Color32::from_rgb(172, 55, 52)
                        }),
                    );
                    ui.label(&v.detail);
                });
        }
    }
    fn board_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("证书记录");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("清空").clicked() {
                    self.confirm_clear = true;
                }
            });
        });
        ui.label(RichText::new("仅本机记录").size(14.0).color(MUTED));
        ui.horizontal_wrapped(|ui| {
            for q in 1..=3 {
                ui.selectable_value(&mut self.board_q, q, format!("问题 {q}"));
            }
        });
        let mut items: Vec<_> = self
            .records
            .iter()
            .filter(|r| r.question() == self.board_q as u64)
            .cloned()
            .collect();
        if self.board_q != 3 {
            items.sort_by_key(|r| r.score()[0]);
        }
        if items.is_empty() {
            ui.add_space(35.0);
            ui.label(RichText::new("暂无已验证证书").color(MUTED));
        }
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("records")
                .spacing([24.0, 18.0])
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    if self.board_q != 3 {
                        ui.strong("名次");
                    }
                    ui.strong("队伍");
                    ui.strong("总用时 / 分钟");
                    ui.strong("架次");
                    if self.board_q == 3 {
                        ui.strong("临时任务");
                    }
                    ui.strong("证书");
                    ui.end_row();
                    let mut rank = 0;
                    let mut previous = None;
                    for (i, item) in items.iter().enumerate() {
                        let s = item.score();
                        if previous != Some(s[0]) {
                            rank = i + 1;
                            previous = Some(s[0]);
                        }
                        if self.board_q != 3 {
                            ui.label(rank.to_string());
                        }
                        ui.label(item.team());
                        ui.strong(fmt(s[0]));
                        ui.label(fmt(s[3]));
                        if self.board_q == 3 {
                            ui.label(format!("{} / 160", s[7]));
                        }
                        if ui.small_button("PNG ↓").clicked() {
                            self.export(item, ui.ctx());
                        }
                        ui.end_row();
                    }
                });
        });
    }
}
impl eframe::App for FlightApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.task.is_some() {
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
                            .size(22.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.add_space(30.0);
                    for (i, label) in ["生成证书", "验证证书", "证书记录"].iter().enumerate()
                    {
                        ui.selectable_value(&mut self.page, i, *label);
                    }
                    if ui.small_button("说明").clicked() {
                        self.about = true;
                    }
                });
            });
        if self.task.is_some() || !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status")
                .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(18))
                .show(ctx, |ui| {
                    if self.task.is_some() {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label(&self.progress);
                            if ui.button("取消").clicked() {
                                self.cancel();
                            }
                        });
                    } else {
                        ui.colored_label(
                            if self.error {
                                Color32::from_rgb(172, 55, 52)
                            } else {
                                ACCENT
                            },
                            &self.status,
                        );
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(245, 248, 247))
                    .inner_margin(28),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        0 => self.generate_page(ui),
                        1 => self.verify_page(ui),
                        _ => self.board_page(ui),
                    });
            });
        egui::Window::new("使用说明")
            .open(&mut self.about)
            .resizable(false)
            .default_width(510.0)
            .show(ctx, |ui| {
                ui.label("提交航线表和人员分配表，生成后保存 PNG 原图。方案与计算均留在本机。");
                ui.label("导入他人证书可核验成绩。证书证明方案可行，不证明最优性或队伍真实身份。");
                ui.label("最多支持 256 架次。生成较耗内存，网页端建议使用电脑。");
            });
        if self.confirm_clear {
            egui::Window::new("清空记录")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("清空本机证书记录？已保存的图片不受影响。");
                    ui.horizontal(|ui| {
                        if ui.button("清空").clicked() {
                            if self
                                .task
                                .as_ref()
                                .is_some_and(|t| t.action == "verify" || t.action == "import")
                            {
                                self.backend.cancel();
                                self.task = None;
                            }
                            self.records.clear();
                            self.pending.clear();
                            self.verifications.clear();
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
                .filter(|(p, _)| p.original.is_none())
                .map(|(p, _)| p.certificate.clone()),
        );
        if let Some(task) = &self.task {
            if let Some(p) = &task.imported {
                if p.original.is_none() {
                    certs.push(p.certificate.clone());
                }
            }
        }
        eframe::set_value(storage, SAVE_KEY, &certs);
    }
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(3)
    }
}
