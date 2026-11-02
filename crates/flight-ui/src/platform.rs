use eframe::egui;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub enum FileEvent {
    Loaded {
        slot: usize,
        name: String,
        bytes: Vec<u8>,
    },
    Saved(String),
    Error(String),
}
pub type FileQueue = Arc<Mutex<Vec<FileEvent>>>;
fn deliver(queue: &FileQueue, ctx: &egui::Context, event: FileEvent) {
    queue.lock().unwrap().push(event);
    ctx.request_repaint();
}
#[cfg(not(target_arch = "wasm32"))]
pub fn pick(slot: usize, queue: FileQueue, ctx: egui::Context) {
    let task = async move {
        let dialog = if slot == 6 {
            rfd::AsyncFileDialog::new().add_filter("证书", &["png", "jpg", "jpeg"])
        } else {
            rfd::AsyncFileDialog::new().add_filter("CSV", &["csv"])
        };
        if slot == 6 {
            if let Some(files) = dialog.pick_files().await {
                if files.len() > 20 {
                    deliver(
                        &queue,
                        &ctx,
                        FileEvent::Error("每次最多导入 20 份证书".into()),
                    );
                    return;
                }
                for f in files {
                    deliver(
                        &queue,
                        &ctx,
                        FileEvent::Loaded {
                            slot,
                            name: f.file_name(),
                            bytes: f.read().await,
                        },
                    );
                }
            }
        } else if let Some(f) = dialog.pick_file().await {
            deliver(
                &queue,
                &ctx,
                FileEvent::Loaded {
                    slot,
                    name: f.file_name(),
                    bytes: f.read().await,
                },
            );
        }
    };
    spawn(task);
}
#[cfg(not(target_arch = "wasm32"))]
pub fn save(name: String, bytes: Vec<u8>, queue: FileQueue, ctx: egui::Context) {
    spawn(async move {
        if let Some(f) = rfd::AsyncFileDialog::new()
            .set_file_name(&name)
            .save_file()
            .await
        {
            match f.write(&bytes).await {
                Ok(()) => deliver(&queue, &ctx, FileEvent::Saved(name)),
                Err(e) => deliver(&queue, &ctx, FileEvent::Error(format!("保存失败：{e}"))),
            }
        }
    });
}
#[cfg(target_arch = "wasm32")]
mod browser_files {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(js_namespace = flightBridge)]
    extern "C" {
        #[wasm_bindgen(catch, js_name = chooseFiles)]
        async fn choose_files(slot: u32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = downloadFile)]
        fn download_file(name: &str, bytes: &[u8]) -> Result<(), JsValue>;
    }
    pub fn pick(slot: usize, queue: FileQueue, ctx: egui::Context) {
        spawn(async move {
            let result = async {
                let value = choose_files(slot as u32)
                    .await
                    .map_err(|e| e.as_string().unwrap_or_else(|| "文件读取失败".into()))?;
                let files: Vec<Value> =
                    serde_json::from_str(&value.as_string().ok_or("文件读取失败")?)
                        .map_err(|e| e.to_string())?;
                for file in files {
                    let bytes = STANDARD
                        .decode(file["data"].as_str().ok_or("文件读取失败")?)
                        .map_err(|e| e.to_string())?;
                    deliver(
                        &queue,
                        &ctx,
                        FileEvent::Loaded {
                            slot,
                            name: file["name"].as_str().ok_or("文件名无效")?.into(),
                            bytes,
                        },
                    );
                }
                Ok::<(), String>(())
            }
            .await;
            if let Err(e) = result {
                deliver(&queue, &ctx, FileEvent::Error(e));
            }
        });
    }
    pub fn save(name: String, bytes: Vec<u8>, queue: FileQueue, ctx: egui::Context) {
        let event = match download_file(&name, &bytes) {
            Ok(()) => FileEvent::Saved(name),
            Err(e) => FileEvent::Error(format!("下载失败：{e:?}")),
        };
        deliver(&queue, &ctx, event);
    }
}
#[cfg(target_arch = "wasm32")]
pub use browser_files::{pick, save};

#[cfg(target_arch = "wasm32")]
fn spawn(f: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(f);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn(f: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(f));
}

#[cfg(target_arch = "wasm32")]
mod target {
    use super::*;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(js_namespace=flightBridge)]
    extern "C" {
        #[wasm_bindgen(catch)]
        fn send(text: &str) -> Result<(), JsValue>;
        fn drain() -> String;
        fn cancel();
    }
    pub struct Backend;
    impl Backend {
        pub fn new() -> Self {
            Self
        }
        pub fn send(&mut self, value: Value) -> Result<(), String> {
            send(&value.to_string()).map_err(|e| format!("计算引擎启动失败：{e:?}"))
        }
        pub fn drain(&mut self) -> Vec<Value> {
            serde_json::from_str(&drain()).unwrap_or_default()
        }
        pub fn cancel(&mut self) {
            cancel();
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
mod target {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    pub struct Backend {
        child: Option<Child>,
        receiver: Option<Receiver<Value>>,
    }
    impl Backend {
        pub fn new() -> Self {
            Self {
                child: None,
                receiver: None,
            }
        }
        pub fn send(&mut self, value: Value) -> Result<(), String> {
            self.cancel();
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            let mut command = Command::new(executable);
            command
                .arg(if value["action"] == "import" {
                    "--import"
                } else {
                    "--engine"
                })
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .spawn()
                .map_err(|e| format!("无法启动 Rust 计算引擎：{e}"))?;
            writeln!(child.stdin.take().ok_or("无法打开引擎输入")?, "{value}")
                .map_err(|e| e.to_string())?;
            let stdout = child.stdout.take().ok_or("无法打开引擎输出")?;
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let mut complete = false;
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if let Ok(message) = serde_json::from_str::<Value>(&line) {
                        if message["type"] == "result" || message["type"] == "error" {
                            complete = true;
                        }
                        let _ = sender.send(message);
                    }
                }
                if !complete {
                    let _=sender.send(serde_json::json!({"type":"error","message":"本地计算进程中断，请检查可用内存和发行包完整性"}));
                }
            });
            self.child = Some(child);
            self.receiver = Some(receiver);
            Ok(())
        }
        pub fn drain(&mut self) -> Vec<Value> {
            self.receiver
                .as_ref()
                .map(|r| r.try_iter().collect())
                .unwrap_or_default()
        }
        pub fn cancel(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.receiver = None;
        }
    }
    impl Drop for Backend {
        fn drop(&mut self) {
            self.cancel();
        }
    }
}
pub use target::Backend;
