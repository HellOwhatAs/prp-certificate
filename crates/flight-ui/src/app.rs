use crate::certificate::{self, Imported, Verified, fmt};
use crate::platform::{self, Backend, FileEvent, FileQueue};
use base64::{Engine, engine::general_purpose::STANDARD};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
const INK: Color32 = Color32::from_rgb(29, 54, 59);
const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
const RED: Color32 = Color32::from_rgb(166, 54, 47);
const NAMES: [&str; 3] = [
    "第一问 · 出海运输",
    "第二问 · 双向运输",
    "第三问 · 多日排班",
];
const SLOT_KEYS: [&str; 2] = ["routes", "assignments"];
const SAVE_KEY: &str = "prp-combined-certificates-v1";
const DRAFT_KEY: &str = "prp-generation-draft-v1";
const MAX_RECORDS: usize = 200;

// Only public proofs and enough metadata to resume are persisted. CSV names,
// contents and unchecked scores deliberately remain in memory.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StoredDraft {
    team: String,
    editing_record: Option<String>,
    active: [bool; 3],
    proofs: [Option<Value>; 3],
}

#[derive(Default)]
struct CsvFile {
    name: String,
    text: String,
}
#[derive(Default)]
struct Question {
    requested: bool,
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
        self.requested || self.has_files() || self.proof.is_some()
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
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportSource {
    File,
    Record,
    Draft(usize),
}
struct Pending {
    imported: Imported,
    name: String,
    source: ImportSource,
}
struct Batch {
    checks: VecDeque<usize>,
    proofs: VecDeque<usize>,
}
struct Review {
    name: Option<String>,
    item: Verified,
    texture: egui::TextureHandle,
}
struct ImportFailure {
    name: String,
    message: String,
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
    draft_texture: Option<egui::TextureHandle>,
    review: Option<Review>,
    import_failures: Vec<ImportFailure>,
    ready: Option<Verified>,
    dirty: bool,
    checkpoint_pending: bool,
    confirm_clear: bool,
    imports_paused: bool,
    storage_open: bool,
    storage_busy: bool,
    storage_state: Option<Value>,
    storage_error: String,
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
        style.visuals.panel_fill = Color32::from_rgb(245, 248, 247);
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.text_edit_bg_color = Some(Color32::from_rgb(241, 247, 244));
        style.visuals.selection.bg_fill = Color32::from_rgb(211, 233, 226);
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(234, 240, 237);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(234, 240, 237);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(211, 233, 226);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(211, 233, 226);
        style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(190, 219, 208);
        style.visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0_f32, Color32::from_rgb(195, 213, 205));
        style.visuals.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.0_f32, Color32::from_rgb(96, 156, 142));
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
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
        let mut app = Self {
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
            pending: VecDeque::new(),
            draft_texture: None,
            review: None,
            import_failures: vec![],
            ready: None,
            dirty: true,
            checkpoint_pending: false,
            confirm_clear: false,
            imports_paused: false,
            storage_open: false,
            storage_busy: false,
            storage_state: None,
            storage_error: String::new(),
        };
        if let Some(storage) = cc.storage {
            app.restore_state(storage);
        }
        app
    }
    fn restore_state(&mut self, storage: &dyn eframe::Storage) {
        let draft: StoredDraft = eframe::get_value(storage, DRAFT_KEY).unwrap_or_default();
        self.team = draft.team;
        self.editing_record = draft.editing_record;
        for (q, certificate) in draft.proofs.into_iter().enumerate() {
            self.questions[q].requested = draft.active[q] || certificate.is_some();
            if let Some(certificate) = certificate {
                // Parsing only establishes the expected slot/team. No stored proof
                // becomes usable until the normal backend verification succeeds.
                let matches_draft = flight_format::CertificateSet::from_value(&certificate)
                    .is_ok_and(|set| {
                        set.certificates.len() == 1
                            && set.certificates[0].metadata.question == q + 1
                            && set.team() == self.team.trim()
                    });
                if matches_draft {
                    self.pending.push_back(Pending {
                        imported: Imported {
                            certificate,
                            original: None,
                        },
                        name: format!("第 {} 问已保存进度", q + 1),
                        source: ImportSource::Draft(q),
                    });
                } else {
                    self.questions[q].error =
                        "已保存进度与当前题目或署名不一致，请重新选择 CSV".into();
                }
            }
        }
        if self.questions.iter().any(Question::active) {
            self.status = if self.pending.is_empty() {
                "已恢复生成进度，请重新选择未完成题目的 CSV"
            } else {
                "正在恢复生成进度，未完成的题目需重新选择 CSV"
            }
            .into();
        }
        let saved: Vec<Value> = eframe::get_value(storage, SAVE_KEY).unwrap_or_default();
        self.pending.extend(
            saved
                .into_iter()
                .take(MAX_RECORDS)
                .map(|certificate| Pending {
                    imported: Imported {
                        certificate,
                        original: None,
                    },
                    name: "已保存证书".into(),
                    source: ImportSource::Record,
                }),
        );
    }
    fn stored_records(&self) -> Vec<Value> {
        // records and the recovery queue are both newest-first. The in-flight
        // record precedes the queue, including when saving during verification.
        let mut certs: Vec<Value> = self.records.iter().map(|r| r.certificate.clone()).collect();
        certs.extend(
            self.task
                .iter()
                .filter_map(|task| task.import.as_ref())
                .chain(self.pending.iter())
                .filter(|p| p.source == ImportSource::Record)
                .map(|p| p.imported.certificate.clone()),
        );
        let mut seen = HashSet::new();
        certs.retain(|value| {
            flight_format::CertificateSet::from_value(value)
                .and_then(|set| set.id())
                .map_or(true, |id| seen.insert(id))
        });
        certs.truncate(MAX_RECORDS);
        certs
    }
    fn stored_draft(&self) -> StoredDraft {
        let mut proofs = std::array::from_fn(|q| {
            self.questions[q]
                .proof_for(&self.team)
                .map(|p| p.certificate.clone())
        });
        // Keep unverified recovery work across another close/cancel, without
        // promoting it to trusted Question state or dropping completed work.
        for p in self
            .task
            .iter()
            .filter_map(|task| task.import.as_ref())
            .chain(self.pending.iter())
        {
            if let ImportSource::Draft(q) = p.source {
                let same_team = flight_format::CertificateSet::from_value(&p.imported.certificate)
                    .is_ok_and(|set| set.team() == self.team.trim());
                if proofs[q].is_none() && self.questions[q].active() && same_team {
                    proofs[q] = Some(p.imported.certificate.clone());
                }
            }
        }
        StoredDraft {
            team: self.team.clone(),
            editing_record: self.editing_record.clone(),
            active: std::array::from_fn(|q| self.questions[q].active()),
            proofs,
        }
    }
    fn save_state(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SAVE_KEY, &self.stored_records());
        eframe::set_value(storage, DRAFT_KEY, &self.stored_draft());
    }
    fn flush_checkpoint(&mut self, storage: &mut dyn eframe::Storage) {
        if self.checkpoint_pending {
            self.save_state(storage);
            storage.flush();
            self.checkpoint_pending = false;
        }
    }
    fn discard_draft_recovery(&mut self, q: usize) {
        self.pending.retain(|p| p.source != ImportSource::Draft(q));
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
            self.records.truncate(MAX_RECORDS);
            self.checkpoint_pending = true;
        }
    }
    fn restore_record(&mut self, item: Verified) {
        if !self.records.iter().any(|r| r.id() == item.id()) {
            self.records.push(item);
            self.records.truncate(MAX_RECORDS);
        }
        // Moving a verified record out of the recovery queue leaves the saved
        // order and contents unchanged, so it does not need a checkpoint.
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
        if self.storage_busy {
            self.show_error("正在处理参数缓存，请稍后重试");
            return false;
        }
        match self.backend.send(payload) {
            Ok(()) => {
                self.task = Some(Task { kind, import });
                self.progress = "处理中…".into();
                self.error = false;
                true
            }
            Err(e) => {
                if let Some(import) = import {
                    self.pending.push_front(import);
                    self.imports_paused = true;
                }
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
        if self.busy() || self.storage_busy {
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
        if self.busy() || self.storage_busy {
            return;
        }
        if self
            .pending
            .iter()
            .any(|p| matches!(p.source, ImportSource::Draft(_)))
        {
            self.imports_paused = false;
            self.next_import();
            return;
        }
        let team = self.team.trim();
        if team.is_empty() {
            self.show_error("请输入队伍署名");
            return;
        }
        if team.chars().count() > 40 {
            self.show_error("队伍署名不能超过 40 个字符");
            return;
        }
        if team.chars().any(char::is_control) {
            self.show_error("队伍署名不能包含控制字符");
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
                question.error = if question.proof.is_some() {
                    "队伍署名与证书不一致，请重新选择方案文件"
                } else {
                    "请选择完整的航线表和人员分配表以继续生成"
                }
                .into();
                let error = format!("第 {} 问：{}", q + 1, question.error);
                self.show_error(error);
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
        if self.task.is_some() || self.storage_busy {
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
                self.pending.retain(|p| {
                    p.source != ImportSource::Record
                        || !flight_format::CertificateSet::from_value(&p.imported.certificate)
                            .is_ok_and(|set| {
                                set.team() == item.team()
                                    && set.id().ok().as_deref() == Some(previous.as_str())
                            })
                });
                self.editing_record = Some(item.id());
            }
            self.ready = Some(item.clone());
            self.add_record(item);
            self.status = "证书已生成".into();
            self.error = false;
            if self.storage_open {
                self.request_storage("list", None, ctx);
            }
        } else {
            self.show_error("证书未生成");
        }
        self.dirty = true;
        self.refresh_draft(ctx);
    }
    fn cancel(&mut self, ctx: &egui::Context) {
        self.backend.cancel();
        if let Some(task) = self.task.take() {
            if let Some(import) = task.import {
                self.pending.push_front(import);
            }
        }
        self.batch = None;
        self.imports_paused = true;
        self.checkpoint_pending = true;
        self.status = "已取消".into();
        self.error = false;
        self.progress.clear();
        self.dirty = true;
        if self.storage_open {
            self.request_storage("list", None, ctx);
        }
    }
    fn next_import(&mut self) {
        if self.busy() || self.storage_busy || self.imports_paused {
            return;
        }
        if let Some(p) = self.pending.pop_front() {
            if p.source == ImportSource::File {
                self.review = None;
            }
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
        self.pending
            .retain(|p| !matches!(p.source, ImportSource::Draft(_)));
        self.checkpoint_pending = true;
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
        self.checkpoint_pending = true;
        self.ready = None;
        self.dirty = true;
        self.status.clear();
    }
    fn accept_proof(&mut self, q: usize, item: Verified) -> Result<(), String> {
        let set = item.set()?;
        if set.certificates.len() != 1
            || item.question() != (q + 1) as u64
            || item.team() != self.team.trim()
        {
            return Err("证明结果与当前方案不一致".into());
        }
        self.questions[q].proof = Some(item);
        self.questions[q].error.clear();
        self.checkpoint_pending = true;
        self.dirty = true;
        Ok(())
    }
    fn accept_import(
        &mut self,
        p: &Pending,
        item: Verified,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        item.set()?;
        if !p.imported.certificate.is_null() {
            certificate::check_document(&p.imported, &item)?;
        }
        let item = Verified::combine(&[item])?;
        match p.source {
            ImportSource::Draft(q) => {
                if !self.questions[q].active() || self.questions[q].has_files() {
                    return Err("已保存进度已被当前方案替换".into());
                }
                self.accept_proof(q, item)?;
                let still_restoring = self
                    .pending
                    .iter()
                    .any(|p| matches!(p.source, ImportSource::Draft(_)));
                if !still_restoring {
                    self.ready = self.ready_certificate();
                    if !self.questions.iter().any(|q| !q.error.is_empty()) {
                        self.status = if self.ready.is_some() {
                            "已恢复已验证的证书进度"
                        } else {
                            "已恢复生成进度，请重新选择未完成题目的 CSV"
                        }
                        .into();
                        self.error = false;
                    }
                }
            }
            ImportSource::Record => self.restore_record(item),
            ImportSource::File => {
                self.add_record(item.clone());
                self.set_review(item, Some(p.name.clone()), ctx);
                self.status = "验证通过".into();
                self.error = false;
            }
        }
        Ok(())
    }
    fn reject_import(&mut self, p: Pending, error: String) {
        if let ImportSource::Draft(q) = p.source {
            self.questions[q].error = format!("已保存进度验证失败：{error}；请重新选择 CSV");
            self.dirty = true;
        }
        self.checkpoint_pending = true;
        self.set_review_error(p.name, error);
    }
    fn fail_task(&mut self, task: Task, error: String) {
        match task.kind {
            Kind::Check(q) | Kind::Prove(q) => {
                self.questions[q].error = error.clone();
                self.batch = None;
                self.show_error(format!("第 {} 问：{error}", q + 1));
                self.dirty = true;
                self.checkpoint_pending = true;
            }
            Kind::Import => {
                if let Some(p) = task.import {
                    if let ImportSource::Draft(q) = p.source {
                        // An engine/worker error is not evidence that the stored
                        // proof is invalid. Keep it untrusted and retryable.
                        let message = format!("已保存进度未能验证：{error}；可重试或重新选择 CSV");
                        self.questions[q].error = message.clone();
                        self.pending.push_front(p);
                        self.imports_paused = true;
                        self.checkpoint_pending = true;
                        self.dirty = true;
                        self.show_error(format!("第 {} 问：{message}", q + 1));
                        return;
                    }
                    self.reject_import(p, error.clone());
                }
                self.show_error(error);
            }
        }
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
    fn set_review(&mut self, item: Verified, name: Option<String>, ctx: &egui::Context) {
        match certificate::render(&item) {
            Ok(p) => {
                if name.is_none() {
                    self.import_failures.clear();
                }
                self.review = Some(Review {
                    name,
                    item,
                    texture: Self::texture(ctx, "review", &p),
                });
                self.status.clear();
                self.error = false;
            }
            Err(e) => self.set_review_error(name.unwrap_or_default(), e),
        }
    }
    fn set_review_error(&mut self, name: String, message: String) {
        self.show_error(message.clone());
        self.review = None;
        self.import_failures
            .insert(0, ImportFailure { name, message });
        self.import_failures.truncate(20);
    }
    fn receive(&mut self, ctx: &egui::Context) {
        let files = std::mem::take(&mut *self.file_events.lock().unwrap());
        for event in files {
            match event {
                FileEvent::SelectionStarted { slot: 6 } => self.import_failures.clear(),
                FileEvent::SelectionStarted { .. } => {}
                FileEvent::LoadError {
                    slot: 6,
                    name,
                    message,
                } => {
                    self.set_review_error(name, message);
                }
                FileEvent::LoadError { name, message, .. } => {
                    self.show_error(format!("{name}：{message}"));
                }
                FileEvent::Loaded { slot, name, bytes } if slot == 6 => {
                    if bytes.len() > 16_000_000 {
                        self.set_review_error(name, "证书图片不能超过 16 MB".into());
                    } else {
                        self.pending.push_back(Pending {
                            imported: Imported {
                                certificate: Value::Null,
                                original: Some(bytes),
                            },
                            name,
                            source: ImportSource::File,
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
                            self.discard_draft_recovery(slot / 2);
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
                FileEvent::Storage(result) => {
                    self.storage_busy = false;
                    match result {
                        Ok(state) => {
                            self.storage_state = Some(state);
                            self.storage_error.clear();
                        }
                        Err(e) => self.storage_error = e,
                    }
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
                        self.fail_task(task, error);
                    }
                    self.progress.clear();
                    if self.storage_open {
                        self.request_storage("list", None, ctx);
                    }
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
                            if let Err(e) = serde_json::from_value::<Verified>(result.clone())
                                .map_err(|e| e.to_string())
                                .and_then(|item| self.accept_proof(q, item))
                            {
                                self.batch = None;
                                self.show_error(e);
                            }
                        }
                        Kind::Import => {
                            let p = task.import.unwrap();
                            if let Err(e) = serde_json::from_value::<Verified>(result.clone())
                                .map_err(|e| e.to_string())
                                .and_then(|item| self.accept_import(&p, item, ctx))
                            {
                                self.reject_import(p, e);
                            }
                        }
                    }
                    self.progress.clear();
                    self.next_batch(ctx);
                    if self.storage_open && !self.busy() {
                        self.request_storage("list", None, ctx);
                    }
                }
                _ => {}
            }
        }
        self.next_batch(ctx);
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
    fn section(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) -> egui::Response {
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(207, 221, 215)))
            .corner_radius(8)
            .inner_margin(20)
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                body(ui);
            })
            .response
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
                    egui::Button::new(RichText::new(label).size(14.0)).corner_radius(5),
                )
                .clicked()
            {
                self.template(q, assignment, ui.ctx());
            }
        }
    }
    fn question_buttons(&mut self, ui: &mut egui::Ui, q: usize) {
        // Right-to-left layout keeps the primary check button at the right edge.
        let check = ui
            .add_enabled_ui(
                self.questions[q].has_files() && !self.busy() && !self.storage_busy,
                |ui| {
                    let visuals = ui.visuals_mut();
                    visuals.widgets.inactive.weak_bg_fill = ACCENT;
                    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(23, 133, 119);
                    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(19, 94, 84);
                    ui.add_sized(
                        [64.0, 36.0],
                        egui::Button::new(RichText::new("校验").size(14.0).color(Color32::WHITE))
                            .corner_radius(5),
                    )
                    .clicked()
                },
            )
            .inner;
        if check {
            self.check_question(q);
        }
        let clear = ui
            .add_enabled_ui(self.questions[q].active() && !self.busy(), |ui| {
                ui.add_sized(
                    [64.0, 36.0],
                    egui::Button::new(RichText::new("清空").size(14.0)).corner_radius(5),
                )
                .clicked()
            })
            .inner;
        if clear {
            self.discard_draft_recovery(q);
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
                self.pending
                    .retain(|p| !matches!(p.source, ImportSource::Draft(_)));
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
                    !self.busy()
                        && !self.storage_busy
                        && self.questions.iter().any(Question::active),
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
                        for (label, value) in certificate::score_metrics(scores) {
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
                        if q == 3 {
                            ui.separator();
                            ui.columns(2, |cols| {
                                cols[0].label(
                                    RichText::new("临时任务完成").size(font_size).color(MUTED),
                                );
                                cols[1].with_layout(
                                    egui::Layout::top_down(egui::Align::Max),
                                    |ui| {
                                        ui.label(
                                            RichText::new(certificate::temporary_tasks(scores))
                                                .size(font_size),
                                        );
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
                self.verification_details(&mut cols[0]);
                self.review_panel(&mut cols[1]);
            });
        } else {
            self.verification_details(ui);
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
                ui.painter().rect_filled(row, 6, Color32::WHITE);
                ui.painter().rect_stroke(
                    row,
                    6,
                    egui::Stroke::new(1.0_f32, Color32::from_rgb(207, 221, 215)),
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
    fn record_actions(&mut self, ui: &mut egui::Ui, item: &Verified, area: egui::Rect) {
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
                    egui::Button::new(RichText::new(label).size(14.0))
                        .corner_radius(5)
                        .min_size(rect.size()),
                )
                .clicked();
            if clicked {
                match index {
                    0 => {
                        self.set_review(item.clone(), None, ui.ctx());
                        self.page = 1;
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
    fn request_storage(&mut self, operation: &str, q: Option<usize>, ctx: &egui::Context) {
        if self.storage_busy || (operation != "list" && self.busy()) {
            return;
        }
        self.storage_busy = true;
        self.storage_error.clear();
        platform::storage(operation, q, self.file_events.clone(), ctx.clone());
    }
    fn storage_button(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(40.0, 44.0), egui::Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "存储管理"));
        let visuals = ui.style().interact(&response);
        if response.hovered() || response.has_focus() || self.storage_open {
            ui.painter()
                .rect_filled(rect.shrink(2.0), 5.0, visuals.weak_bg_fill);
        }
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
        if response.on_hover_text("存储管理").clicked() {
            self.storage_open = !self.storage_open;
            if self.storage_open {
                self.request_storage("list", None, ui.ctx());
            }
        }
    }
    fn storage_window(&mut self, ctx: &egui::Context) {
        if !self.storage_open {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("storage-management"))
            .backdrop_color(Color32::from_black_alpha(60))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(Color32::WHITE)
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
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "关闭存储管理")
                });
                if response.hovered()
                    || response.has_focus()
                    || response.is_pointer_button_down_on()
                {
                    let fill = if response.is_pointer_button_down_on() {
                        Color32::from_rgb(211, 225, 218)
                    } else {
                        Color32::from_rgb(231, 239, 235)
                    };
                    ui.painter().rect_filled(close_rect, 5.0, fill);
                }
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
                            let card = egui::Frame::new()
                                .fill(Color32::from_rgb(245, 248, 247))
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    Color32::from_rgb(211, 225, 218),
                                ))
                                .corner_radius(8)
                                .inner_margin(14);
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
                                                .corner_radius(5)
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
    fn navigation_buttons(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.spacing_mut().button_padding = egui::vec2(8.0, 8.0);
        for (i, label) in ["生成证书", "验证证书", "证书记录"].iter().enumerate() {
            if ui
                .add_sized(
                    [width, 44.0],
                    egui::Button::selectable(
                        self.page == i,
                        RichText::new(*label).size(if width < 80.0 { 14.0 } else { 16.0 }),
                    ),
                )
                .clicked()
            {
                self.page = i;
            }
        }
    }
    fn navigation(&mut self, ui: &mut egui::Ui) {
        const TITLE: &str = "海上油田人员直升机运载计划编排";
        let title = || RichText::new(TITLE).color(ACCENT).size(21.0).strong();
        let title_width = ui
            .painter()
            .layout_no_wrap(TITLE.into(), egui::FontId::proportional(21.0), ACCENT)
            .size()
            .x;
        let width = ui.available_width();
        if width >= title_width + 32.0 + 3.0 * 96.0 + 24.0 + 56.0 {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 44.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(title());
                    ui.add_space(20.0);
                    self.navigation_buttons(ui, 96.0);
                    ui.add_space((ui.available_width() - 44.0).max(0.0));
                    self.storage_button(ui);
                },
            );
        } else {
            ui.label(title());
            ui.allocate_ui_with_layout(
                egui::vec2(width, 44.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    self.navigation_buttons(ui, ((width - 80.0) / 3.0).min(120.0));
                    ui.add_space((ui.available_width() - 44.0).max(0.0));
                    self.storage_button(ui);
                },
            );
        }
    }
}
impl eframe::App for FlightApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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
                if self.storage_open {
                    ui.disable();
                }
                self.navigation(ui);
            });
        if self.busy() || !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status")
                .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(14))
                .show(ctx, |ui| {
                    if self.storage_open {
                        ui.disable();
                    }
                    if self.busy() {
                        // Reserve the complete row before placing its differently sized widgets.
                        // Long progress messages yield space to the cancel button, never another row.
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 40.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().button_padding = egui::vec2(14.0, 8.0);
                                ui.add(egui::Spinner::new().size(18.0));
                                let text_width =
                                    (ui.available_width() - 72.0 - ui.spacing().item_spacing.x)
                                        .max(0.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(text_width, 40.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.add(egui::Label::new(&self.progress).truncate());
                                    },
                                );
                                if ui
                                    .add_sized([72.0, 40.0], egui::Button::new("取消"))
                                    .clicked()
                                {
                                    self.cancel(ctx);
                                }
                            },
                        );
                    } else {
                        ui.colored_label(if self.error { RED } else { ACCENT }, &self.status);
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(245, 248, 247))
                    .inner_margin(egui::Margin::symmetric(24, 0)),
            )
            .show(ctx, |ui| {
                if self.storage_open {
                    ui.disable();
                }
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("page-scroll", self.page))
                    .auto_shrink([false, false]);
                if std::mem::take(&mut self.reset_scroll) {
                    scroll = scroll.vertical_scroll_offset(0.0);
                }
                scroll.show(ui, |ui| {
                    // Vertical breathing room belongs to the scrolling content, not its viewport.
                    ui.add_space(24.0);
                    match self.page {
                        0 => self.generate_page(ui),
                        1 => self.verify_page(ui),
                        _ => self.board_page(ui),
                    }
                    ui.add_space(24.0);
                });
            });
        self.storage_window(ctx);
        if self.confirm_clear {
            egui::Window::new("清空记录")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if self.storage_open {
                        ui.disable();
                    }
                    ui.label("清空本机证书记录？");
                    ui.horizontal(|ui| {
                        if ui.button("清空").clicked() {
                            self.records.clear();
                            self.pending
                                .retain(|p| matches!(p.source, ImportSource::Draft(_)));
                            self.checkpoint_pending = true;
                            self.review = None;
                            self.import_failures.clear();
                            self.confirm_clear = false;
                        }
                        if ui.button("取消").clicked() {
                            self.confirm_clear = false;
                        }
                    });
                });
        }
        if let Some(storage) = frame.storage_mut() {
            self.flush_checkpoint(storage);
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_modal_blocks_navigation_and_close_has_a_full_hoverable_hit_area() {
        use eframe::App;
        let ctx = egui::Context::default();
        let mut app = FlightApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        ctx.style_mut(|style| style.animation_time = 0.0);
        let draw = |app: &mut FlightApp, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1024.0, 768.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let output = draw(&mut app, vec![]);
        let navigation = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "验证证书" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("navigation button");
        let pointer = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        app.storage_open = true;
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(navigation),
                pointer(navigation, true),
            ],
        );
        draw(&mut app, vec![pointer(navigation, false)]);
        assert_eq!(app.page, 0, "the modal must block background navigation");
        app.storage_open = true;
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let close = ctx
            .read_response(egui::Id::new("storage-close"))
            .unwrap()
            .rect;
        assert_eq!(close.size(), egui::vec2(44.0, 44.0));
        let corner = close.left_top() + egui::vec2(3.0, 3.0);
        let hovered = draw(&mut app, vec![egui::Event::PointerMoved(corner)]);
        assert!(
            hovered.shapes.iter().any(|shape| matches!(
                &shape.shape, egui::Shape::Rect(rect)
                    if rect.rect == close && rect.fill == Color32::from_rgb(231, 239, 235)
            )),
            "hover fills the whole close button"
        );
        draw(&mut app, vec![pointer(corner, true)]);
        draw(&mut app, vec![pointer(corner, false)]);
        assert!(
            !app.storage_open,
            "clicking outside the X but inside its button closes the modal"
        );
        draw(&mut app, vec![egui::Event::PointerMoved(navigation)]);
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(navigation),
                pointer(navigation, true),
            ],
        );
        draw(&mut app, vec![pointer(navigation, false)]);
        assert_eq!(app.page, 1, "background navigation resumes after closing");
    }

    #[test]
    fn storage_window_fits_narrow_and_wide_views() {
        for width in [320.0, 400.0, 1024.0] {
            let ctx = egui::Context::default();
            let mut app = FlightApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
            app.storage_open = true;
            app.storage_state = Some(json!({"available":true,"rows":[
                {"q":1,"bytes":228524311,"cached_bytes":228524311,"total_bytes":228524311},
                {"q":2,"bytes":4194304,"cached_bytes":4194304,"total_bytes":428391940},
                {"q":3,"bytes":0,"cached_bytes":0,"total_bytes":455938453}
            ]}));
            ctx.style_mut(|style| style.animation_time = 0.0);
            let mut output = None;
            for _ in 0..3 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 768.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::TopBottomPanel::top("navigation")
                            .frame(
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .inner_margin(egui::Margin::symmetric(24, 18)),
                            )
                            .show(ctx, |ui| app.navigation(ui));
                        egui::CentralPanel::default().show(ctx, |_| {});
                        app.storage_window(ctx);
                    },
                ));
            }
            fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::Shape::Text(text) => {
                        out.push((text.galley.text().to_owned(), text.visual_bounding_rect()))
                    }
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            texts(shape, out);
                        }
                    }
                    _ => {}
                }
            }
            let output = output.unwrap();
            let mut labels = vec![];
            for shape in &output.shapes {
                texts(&shape.shape, &mut labels);
            }
            for expected in [
                "存储管理",
                "第一问",
                "第二问",
                "第三问",
                "已缓存 228.5 MB",
                "部分缓存 4.2 / 428.4 MB",
                "未缓存",
            ] {
                assert!(
                    labels.iter().any(|(text, _)| text == expected),
                    "{width}: missing {expected}"
                );
            }
            for (text, rect) in &labels {
                assert!(
                    rect.left() >= -1.0 && rect.right() <= width + 1.0,
                    "{width}: clipped {text} at {rect:?}"
                );
            }
        }
    }
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

    #[test]
    fn batch_file_read_failures_survive_later_frames_and_successful_imports() {
        let ctx = egui::Context::default();
        let mut app = application(None);
        // Hold backend work while delivering one picker selection over two frames.
        app.storage_busy = true;
        app.file_events.lock().unwrap().extend([
            FileEvent::SelectionStarted { slot: 6 },
            FileEvent::LoadError {
                slot: 6,
                name: "oversized.png".into(),
                message: "证书图片不能超过 16 MB".into(),
            },
        ]);
        app.receive(&ctx);
        assert_eq!(app.import_failures.len(), 1);
        app.file_events.lock().unwrap().push(FileEvent::Loaded {
            slot: 6,
            name: "valid.png".into(),
            bytes: vec![],
        });
        app.receive(&ctx);
        assert_eq!(
            app.import_failures.len(),
            1,
            "later files must not reset the batch"
        );
        let pending = app.pending.pop_front().unwrap();
        // As in the recovery tests, this is an explicit synthetic engine result.
        app.accept_import(&pending, proof(1), &ctx).unwrap();
        assert!(app.review.is_some());
        assert_eq!(app.import_failures[0].name, "oversized.png");
        assert!(app.import_failures[0].message.contains("16 MB"));
        app.file_events
            .lock()
            .unwrap()
            .push(FileEvent::SelectionStarted { slot: 6 });
        app.receive(&ctx);
        assert!(
            app.import_failures.is_empty(),
            "a new selection resets old failures once"
        );
    }

    #[derive(Default)]
    struct MemoryStorage {
        values: std::collections::HashMap<String, String>,
        flushes: usize,
    }
    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.to_owned(), value);
        }
        fn flush(&mut self) {
            self.flushes += 1;
        }
    }
    fn application(storage: Option<&dyn eframe::Storage>) -> FlightApp {
        let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        cc.storage = storage;
        FlightApp::new(&cc)
    }
    fn distinct_proof(q: usize, n: u16, team: &str) -> Verified {
        let mut item = proof(q);
        let mut nonce = [0; 16];
        nonce[..2].copy_from_slice(&n.to_le_bytes());
        item.certificate["certificates"][0]["metadata"]["nonce"] =
            json!(flight_format::nonce(&nonce));
        item.certificate["certificates"][0]["metadata"]["team"] = json!(team);
        item
    }
    // Synthetic engine responses exercise UI state transitions only. They must
    // never be accepted merely by loading storage: every test explicitly sends
    // a verification success/failure through the same completion handlers.
    fn verification_result(document: &Value) -> Verified {
        let set = flight_format::CertificateSet::from_value(document).unwrap();
        Verified {
            certificate: document.clone(),
            scores: set
                .certificates
                .iter()
                .map(|c| c.scores.iter().map(|v| *v as u64).collect())
                .collect(),
        }
    }
    fn finish_next_recovery(app: &mut FlightApp) {
        let p = app.pending.pop_front().unwrap();
        let result = verification_result(&p.imported.certificate);
        app.accept_import(&p, result, &egui::Context::default())
            .unwrap();
    }
    fn empty_batch() -> Batch {
        Batch {
            checks: VecDeque::new(),
            proofs: VecDeque::new(),
        }
    }
    fn csv_pair(marker: &str) -> [CsvFile; 2] {
        std::array::from_fn(|slot| CsvFile {
            name: format!("PRIVATE-{slot}.csv"),
            text: marker.to_owned(),
        })
    }
    #[test]
    fn record_order_survives_repeated_restart_and_in_flight_save() {
        let mut storage = MemoryStorage::default();
        let expected: Vec<_> = [3, 2, 1]
            .map(|n| distinct_proof(1, n, "甲").certificate)
            .to_vec();
        eframe::set_value(&mut storage, SAVE_KEY, &expected);
        for _ in 0..3 {
            let mut app = application(Some(&storage));
            assert!(
                app.records.is_empty(),
                "stored records require verification"
            );
            finish_next_recovery(&mut app);
            assert!(
                !app.checkpoint_pending,
                "restoring unchanged records does not rewrite storage"
            );
            let p = app.pending.pop_front().unwrap();
            app.task = Some(Task {
                kind: Kind::Import,
                import: Some(p),
            });
            app.save_state(&mut storage);
            assert_eq!(
                eframe::get_value::<Vec<Value>>(&storage, SAVE_KEY).unwrap(),
                expected
            );
            app.cancel(&egui::Context::default());
            app.flush_checkpoint(&mut storage);
            assert_eq!(
                eframe::get_value::<Vec<Value>>(&storage, SAVE_KEY).unwrap(),
                expected
            );
            while !app.pending.is_empty() {
                finish_next_recovery(&mut app);
            }
            assert_eq!(
                app.records
                    .iter()
                    .map(|r| r.certificate.clone())
                    .collect::<Vec<_>>(),
                expected
            );
            app.save_state(&mut storage);
        }
    }
    #[test]
    fn record_limit_keeps_newest_after_restoring_and_adding() {
        let mut storage = MemoryStorage::default();
        let expected: Vec<_> = (1..=MAX_RECORDS as u16)
            .rev()
            .map(|n| distinct_proof(1, n, "甲").certificate)
            .collect();
        eframe::set_value(&mut storage, SAVE_KEY, &expected);
        let mut app = application(Some(&storage));
        while !app.pending.is_empty() {
            finish_next_recovery(&mut app);
        }
        let newest = distinct_proof(1, 201, "甲");
        app.add_record(newest.clone());
        assert_eq!(app.records.len(), MAX_RECORDS);
        assert_eq!(app.records[0].id(), newest.id());
        assert_eq!(
            app.records.last().unwrap().id(),
            distinct_proof(1, 2, "甲").id()
        );
        let saved = app.stored_records();
        assert_eq!(saved[1], expected[0]);
        assert!(!saved.contains(expected.last().unwrap()));
    }
    #[test]
    fn partial_batch_checkpoints_public_proofs_without_csv_and_resumes_after_cancel() {
        let ctx = egui::Context::default();
        let mut storage = MemoryStorage::default();
        let mut app = application(None);
        let original = distinct_proof(1, 10, "甲");
        app.add_record(original.clone());
        app.use_certificate(&original).unwrap();
        app.questions[0].changed();
        app.questions[0].files = csv_pair("SECRET CSV ROUTES AND ASSIGNMENTS");
        app.questions[1].files = csv_pair("SECRET REMAINING CSV");
        let completed = distinct_proof(1, 11, "甲");
        app.accept_proof(0, completed.clone()).unwrap();
        app.batch = Some(empty_batch());
        app.task = Some(Task {
            kind: Kind::Prove(1),
            import: None,
        });
        app.flush_checkpoint(&mut storage);
        assert_eq!(
            storage.flushes, 1,
            "completed proof is flushed before the batch ends"
        );
        app.flush_checkpoint(&mut storage);
        assert_eq!(storage.flushes, 1, "unchanged frames do not keep flushing");
        let serialized = storage.values.values().cloned().collect::<String>();
        assert!(!serialized.contains("SECRET"));
        assert!(!serialized.contains("PRIVATE"));
        let saved: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert_eq!(saved.active, [true, true, false]);
        assert_eq!(saved.proofs[0], Some(completed.certificate.clone()));
        assert!(saved.proofs[1].is_none());
        assert_eq!(
            app.records[0].id(),
            original.id(),
            "partial success preserves the original"
        );

        app.cancel(&ctx);
        app.flush_checkpoint(&mut storage);
        let mut reopened = application(Some(&storage));
        assert_eq!(reopened.team, "甲");
        assert!(reopened.ready.is_none());
        assert!(
            reopened
                .questions
                .iter()
                .all(|q| q.proof.is_none() && !q.has_files())
        );
        assert!(
            reopened.questions[1].active(),
            "unfinished question cannot silently disappear"
        );
        assert!(matches!(
            reopened.pending.front().unwrap().source,
            ImportSource::Draft(0)
        ));
        finish_next_recovery(&mut reopened);
        assert_eq!(
            reopened.questions[0].proof.as_ref().unwrap().id(),
            completed.id()
        );
        assert!(reopened.ready.is_none(), "remaining CSV is still required");
        finish_next_recovery(&mut reopened);
        assert_eq!(reopened.records[0].id(), original.id());
        reopened.generate_certificate(&ctx);
        assert!(reopened.task.is_none());
        assert!(reopened.status.contains("请选择完整"));
        assert_eq!(reopened.records[0].id(), original.id());

        reopened
            .accept_proof(1, distinct_proof(2, 12, "甲"))
            .unwrap();
        reopened.batch = Some(empty_batch());
        reopened.next_batch(&ctx);
        assert_eq!(reopened.records.len(), 1);
        assert_ne!(reopened.records[0].id(), original.id());
        assert!(reopened.records[0].score_for(1).is_some());
        assert!(reopened.records[0].score_for(2).is_some());
    }
    #[test]
    fn later_proof_failure_preserves_completed_progress_and_original_record() {
        let mut app = application(None);
        let original = distinct_proof(1, 20, "甲");
        app.add_record(original.clone());
        app.use_certificate(&original).unwrap();
        app.questions[1].files = csv_pair("PRIVATE INCOMPLETE INPUT");
        let completed = distinct_proof(1, 21, "甲");
        app.accept_proof(0, completed.clone()).unwrap();
        app.batch = Some(empty_batch());
        app.fail_task(
            Task {
                kind: Kind::Prove(1),
                import: None,
            },
            "证明失败".into(),
        );
        assert!(app.batch.is_none());
        assert_eq!(app.records[0].id(), original.id());
        assert_eq!(
            app.questions[0].proof.as_ref().unwrap().id(),
            completed.id()
        );
        assert!(app.ready_certificate().is_none());
        let mut storage = MemoryStorage::default();
        app.flush_checkpoint(&mut storage);
        let draft: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert_eq!(draft.proofs[0], Some(completed.certificate));
        assert_eq!(draft.active, [true, true, false]);
    }
    #[test]
    fn unverified_recovery_survives_reclose_but_cannot_override_new_input() {
        let mut storage = MemoryStorage::default();
        let doc = distinct_proof(1, 30, "甲").certificate;
        eframe::set_value(
            &mut storage,
            DRAFT_KEY,
            &StoredDraft {
                team: "甲".into(),
                active: [true, true, false],
                proofs: [Some(doc.clone()), None, None],
                ..Default::default()
            },
        );
        let mut app = application(Some(&storage));
        let p = app.pending.pop_front().unwrap();
        app.task = Some(Task {
            kind: Kind::Import,
            import: Some(p),
        });
        app.save_state(&mut storage);
        let saved: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert_eq!(saved.proofs[0], Some(doc.clone()));
        app.cancel(&egui::Context::default());
        assert!(app.questions[0].proof.is_none());
        app.discard_draft_recovery(0);
        app.questions[0].changed();
        app.questions[0].files = csv_pair("PRIVATE REPLACEMENT");
        app.invalidate_generated();
        app.flush_checkpoint(&mut storage);
        let saved: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert!(saved.proofs[0].is_none());
        assert!(app.pending.is_empty());
        assert!(saved.active[0]);
    }
    #[test]
    fn restored_proofs_must_match_slot_team_and_verified_document() {
        let mut storage = MemoryStorage::default();
        eframe::set_value(
            &mut storage,
            DRAFT_KEY,
            &StoredDraft {
                team: "甲".into(),
                active: [true; 3],
                proofs: [
                    Some(distinct_proof(2, 40, "甲").certificate),
                    Some(distinct_proof(2, 41, "乙").certificate),
                    Some(distinct_proof(3, 42, "甲").certificate),
                ],
                ..Default::default()
            },
        );
        let mut app = application(Some(&storage));
        assert!(app.questions.iter().all(|q| q.proof.is_none()));
        assert_eq!(app.pending.len(), 1);
        assert!(!app.questions[0].error.is_empty());
        assert!(!app.questions[1].error.is_empty());
        let p = app.pending.pop_front().unwrap();
        let error = app
            .accept_import(&p, distinct_proof(3, 43, "甲"), &egui::Context::default())
            .unwrap_err();
        assert!(
            app.questions[2].proof.is_none(),
            "a different valid response cannot substitute for the saved proof"
        );
        app.reject_import(p, error);
        assert!(app.ready.is_none());
        assert!(app.records.is_empty());
        assert!(
            app.questions
                .iter()
                .all(|q| q.active() && q.proof.is_none())
        );
        app.flush_checkpoint(&mut storage);
        let saved: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert!(saved.proofs.iter().all(Option::is_none));
    }
    #[test]
    fn draft_engine_failure_preserves_unverified_progress_across_restart_and_retry() {
        let mut storage = MemoryStorage::default();
        let original = distinct_proof(1, 70, "甲");
        let first = distinct_proof(1, 71, "甲");
        let second = distinct_proof(2, 72, "甲");
        eframe::set_value(&mut storage, SAVE_KEY, &vec![original.certificate.clone()]);
        eframe::set_value(
            &mut storage,
            DRAFT_KEY,
            &StoredDraft {
                team: "甲".into(),
                editing_record: Some(original.id()),
                active: [true, true, false],
                proofs: [
                    Some(first.certificate.clone()),
                    Some(second.certificate.clone()),
                    None,
                ],
            },
        );
        let mut app = application(Some(&storage));
        let p = app.pending.pop_front().unwrap();
        app.fail_task(
            Task {
                kind: Kind::Import,
                import: Some(p),
            },
            "计算组件暂时无法加载".into(),
        );
        assert!(
            app.imports_paused,
            "do not repeatedly retry a failing worker"
        );
        assert!(
            !app.busy(),
            "the existing generate button must remain available for retry"
        );
        assert!(app.questions.iter().any(Question::active));
        assert_eq!(
            app.pending.len(),
            3,
            "preserve the failed item and the rest of the queue"
        );
        assert_eq!(
            app.pending.front().unwrap().imported.certificate,
            first.certificate
        );
        assert!(app.questions.iter().all(|q| q.proof.is_none()));
        assert!(app.ready.is_none());
        assert!(
            app.import_failures.is_empty(),
            "component failures are not invalid-certificate results"
        );
        assert!(app.questions[0].error.contains("已保存进度未能验证"));
        assert!(app.status.contains("可重试或重新选择 CSV"));
        app.flush_checkpoint(&mut storage);
        let saved: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert_eq!(saved.proofs[0], Some(first.certificate.clone()));
        assert_eq!(saved.proofs[1], Some(second.certificate.clone()));
        assert_eq!(
            eframe::get_value::<Vec<Value>>(&storage, SAVE_KEY).unwrap(),
            vec![original.certificate.clone()]
        );

        let mut reopened = application(Some(&storage));
        assert!(
            !reopened.imports_paused,
            "reopening automatically retries stored public proofs"
        );
        assert!(reopened.questions.iter().all(|q| q.proof.is_none()));
        assert!(reopened.ready.is_none());
        finish_next_recovery(&mut reopened);
        assert_eq!(
            reopened.questions[0].proof.as_ref().unwrap().id(),
            first.id()
        );
        assert!(
            reopened.ready.is_none(),
            "the other proof is still unverified"
        );
        finish_next_recovery(&mut reopened);
        assert_eq!(
            reopened.questions[1].proof.as_ref().unwrap().id(),
            second.id()
        );
        assert!(reopened.ready.is_some());
        finish_next_recovery(&mut reopened);
        assert_eq!(reopened.records.len(), 1);
        assert_eq!(
            reopened.records[0].id(),
            original.id(),
            "restoring progress does not replace the original record"
        );
        reopened.flush_checkpoint(&mut storage);
        let saved: StoredDraft = eframe::get_value(&storage, DRAFT_KEY).unwrap();
        assert_eq!(saved.proofs[0], Some(first.certificate));
        assert_eq!(saved.proofs[1], Some(second.certificate));
    }
    #[test]
    fn renamed_draft_keeps_original_and_stale_team_proofs_are_not_persisted() {
        let mut app = application(None);
        let original = distinct_proof(1, 50, "甲");
        app.add_record(original.clone());
        app.use_certificate(&original).unwrap();
        app.team = "乙".into();
        app.invalidate_generated();
        assert!(app.stored_draft().proofs[0].is_none());
        assert!(app.accept_proof(0, original.clone()).is_err());
        let renamed = distinct_proof(1, 51, "乙");
        app.accept_proof(0, renamed.clone()).unwrap();
        app.batch = Some(empty_batch());
        app.next_batch(&egui::Context::default());
        assert_eq!(app.records.len(), 2);
        assert_eq!(app.records[0].id(), renamed.id());
        assert_eq!(app.records[1].id(), original.id());
    }
    #[test]
    fn successful_replacement_also_removes_original_waiting_for_reverification() {
        let ctx = egui::Context::default();
        let mut app = application(None);
        let original = distinct_proof(1, 60, "甲");
        app.team = "甲".into();
        app.editing_record = Some(original.id());
        app.pending.push_back(Pending {
            imported: Imported {
                certificate: original.certificate,
                original: None,
            },
            name: "已保存证书".into(),
            source: ImportSource::Record,
        });
        app.imports_paused = true;
        let completed = distinct_proof(1, 61, "甲");
        app.accept_proof(0, completed.clone()).unwrap();
        app.batch = Some(empty_batch());
        app.next_batch(&ctx);
        assert!(app.pending.is_empty());
        assert_eq!(app.stored_records(), vec![completed.certificate]);
    }
}
