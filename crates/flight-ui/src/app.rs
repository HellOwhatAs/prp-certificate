mod storage;
#[cfg(test)]
mod tests;
mod views;

use crate::certificate::{self, Imported, Verified};
use crate::materials::MaterialsPage;
use crate::platform::{self, Backend, FileEvent, FileQueue};
use crate::theme::{self, ACCENT, BACKGROUND, RED, SURFACE};
use base64::{Engine, engine::general_purpose::STANDARD};
use eframe::egui;
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

const NAMES: [&str; 3] = [
    "第一问 · 出海运输",
    "第二问 · 双向运输",
    "第三问 · 多日排班",
];
const SLOT_KEYS: [&str; 2] = ["routes", "assignments"];
const SAVE_KEY: &str = "prp-combined-certificates-v1";
const LEGACY_DRAFT_KEY: &str = "prp-generation-draft-v1";
const MAX_RECORDS: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Page {
    #[default]
    Materials,
    Generate,
    Verify,
    Records,
}
impl Page {
    const NAVIGATION: [(Self, &'static str); 4] = [
        (Self::Materials, "题目资料"),
        (Self::Generate, "解答认证"),
        (Self::Verify, "验证证书"),
        (Self::Records, "证书记录"),
    ];
}

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
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportSource {
    File,
    Record,
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
    page: Page,
    materials: MaterialsPage,
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
        theme::configure(&cc.egui_ctx);
        let mut app = Self {
            page: Page::default(),
            materials: MaterialsPage::default(),
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
        // Only completed records survive a restart. A new generation form is empty.
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
    fn save_state(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SAVE_KEY, &self.stored_records());
        // Discard progress left by the earlier partial-draft implementation.
        storage.set_string(LEGACY_DRAFT_KEY, String::new());
    }
    fn flush_checkpoint(&mut self, storage: &mut dyn eframe::Storage) {
        if self.checkpoint_pending {
            self.save_state(storage);
            storage.flush();
            self.checkpoint_pending = false;
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
        for part in parts {
            let q = part.question() as usize - 1;
            self.questions[q].proof = Some(part);
        }
        self.dirty = true;
        self.reset_scroll = true;
        self.status.clear();
        self.error = false;
        self.page = Page::Generate;
        Ok(())
    }
    fn invalidate_generated(&mut self) {
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
}
impl eframe::App for FlightApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.receive(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.busy() {
            100
        } else {
            350
        }));
        let horizontal_margin = if ctx.content_rect().width() < 600.0 {
            12
        } else {
            24
        };
        egui::TopBottomPanel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(horizontal_margin, 18)),
            )
            .show(ctx, |ui| {
                if self.storage_open {
                    ui.disable();
                }
                self.navigation(ui);
            });
        if self.busy() || !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status")
                .frame(egui::Frame::new().fill(SURFACE).inner_margin(14))
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
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(
                if self.page == Page::Materials {
                    egui::Margin::ZERO
                } else {
                    egui::Margin::symmetric(horizontal_margin, 0)
                },
            ))
            .show(ctx, |ui| {
                if self.storage_open {
                    ui.disable();
                }
                if self.page == Page::Materials {
                    // The reader owns both scroll axes and uses the remaining viewport height.
                    if let Some((name, bytes)) = self.materials.ui(ui) {
                        platform::save(
                            name.into(),
                            bytes.to_vec(),
                            self.file_events.clone(),
                            ctx.clone(),
                        );
                    }
                    return;
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
                        Page::Materials => unreachable!(),
                        Page::Generate => self.generate_page(ui),
                        Page::Verify => self.verify_page(ui),
                        Page::Records => self.board_page(ui),
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
                            self.pending.clear();
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
