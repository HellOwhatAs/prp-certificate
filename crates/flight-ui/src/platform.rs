use eframe::egui;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub enum FileEvent {
    SelectionStarted {
        slot: usize,
    },
    Loaded {
        slot: usize,
        name: String,
        bytes: Vec<u8>,
    },
    LoadError {
        slot: usize,
        name: String,
        message: String,
    },
    Saved(String),
    Storage(Result<Value, String>),
    Error(String),
}
pub type FileQueue = Arc<Mutex<Vec<FileEvent>>>;
fn deliver(queue: &FileQueue, ctx: &egui::Context, event: FileEvent) {
    queue.lock().unwrap().push(event);
    ctx.request_repaint();
}
#[cfg(not(target_arch = "wasm32"))]
fn file_limit(slot: usize) -> (usize, &'static str) {
    if slot == 6 {
        (16_000_000, "证书图片不能超过 16 MB")
    } else {
        (10_000_000, "单个 CSV 不能超过 10 MB")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_limited(
    reader: impl std::io::Read,
    limit: usize,
    too_large: &str,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("文件读取失败：{e}"))?;
    if bytes.len() > limit {
        return Err(too_large.into());
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn load_file(file: &rfd::FileHandle, slot: usize) -> FileEvent {
    let name = file.file_name();
    let result = (|| {
        let (limit, too_large) = file_limit(slot);
        let input = std::fs::File::open(file.path()).map_err(|e| format!("无法打开文件：{e}"))?;
        let metadata = input
            .metadata()
            .map_err(|e| format!("无法读取文件信息：{e}"))?;
        if !metadata.is_file() {
            return Err("请选择普通文件".into());
        }
        if metadata.len() > limit as u64 {
            return Err(too_large.into());
        }
        // Also bound the read in case the file grows after its metadata was checked.
        read_limited(input, limit, too_large)
    })();
    match result {
        Ok(bytes) => FileEvent::Loaded { slot, name, bytes },
        Err(message) => FileEvent::LoadError {
            slot,
            name,
            message,
        },
    }
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
                if !files.is_empty() {
                    deliver(&queue, &ctx, FileEvent::SelectionStarted { slot });
                }
                for f in files {
                    deliver(&queue, &ctx, load_file(&f, slot));
                }
            }
        } else if let Some(f) = dialog.pick_file().await {
            deliver(&queue, &ctx, load_file(&f, slot));
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
                if !files.is_empty() {
                    deliver(&queue, &ctx, FileEvent::SelectionStarted { slot });
                }
                for file in files {
                    let name = file["name"].as_str().ok_or("文件名无效")?.to_owned();
                    let bytes = file["data"]
                        .as_str()
                        .ok_or_else(|| "文件读取失败".to_owned())
                        .and_then(|data| STANDARD.decode(data).map_err(|e| e.to_string()));
                    let event = match bytes {
                        Ok(bytes) => FileEvent::Loaded { slot, name, bytes },
                        Err(message) => FileEvent::LoadError {
                            slot,
                            name,
                            message,
                        },
                    };
                    deliver(&queue, &ctx, event);
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
#[cfg(all(not(target_arch = "wasm32"), not(test)))]
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
// Drive the real UI event handlers without spawning the test binary as an engine.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod target {
    use super::*;
    #[derive(Default)]
    pub struct Backend {
        pub requests: Vec<Value>,
        messages: Vec<Value>,
    }
    impl Backend {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn send(&mut self, value: Value) -> Result<(), String> {
            self.cancel();
            self.requests.push(value);
            Ok(())
        }
        pub fn drain(&mut self) -> Vec<Value> {
            std::mem::take(&mut self.messages)
        }
        pub fn cancel(&mut self) {
            self.messages.clear();
        }
        pub fn reply(&mut self, message: Value) {
            self.messages.push(message);
        }
    }
}
pub use target::Backend;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod file_tests {
    use super::read_limited;
    use std::io::{self, Cursor, Read};

    #[test]
    fn file_reads_accept_the_limit_and_stop_when_the_file_has_grown() {
        assert_eq!(
            read_limited(Cursor::new(b"12345678"), 8, "too large").unwrap(),
            b"12345678"
        );
        let mut growing = Cursor::new(vec![0; 1024]);
        assert_eq!(
            read_limited(&mut growing, 8, "too large").unwrap_err(),
            "too large"
        );
        assert_eq!(
            growing.position(),
            9,
            "never read the rest of an oversized file"
        );
        assert!(
            read_limited(Cursor::new([]), 8, "too large")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn file_read_errors_are_reported_instead_of_becoming_empty_files() {
        struct Unreadable;
        impl Read for Unreadable {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "permission denied",
                ))
            }
        }
        assert!(
            read_limited(Unreadable, 8, "too large")
                .unwrap_err()
                .contains("permission denied")
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage(operation: &str, q: Option<usize>, queue: FileQueue, ctx: egui::Context) {
    let operation = operation.to_owned();
    std::thread::spawn(move || {
        deliver(
            &queue,
            &ctx,
            FileEvent::Storage(flight_core::native_parameters::manage(&operation, q)),
        );
    });
}
#[cfg(target_arch = "wasm32")]
pub fn storage(operation: &str, q: Option<usize>, queue: FileQueue, ctx: egui::Context) {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(js_namespace = flightBridge)]
    extern "C" {
        #[wasm_bindgen(catch, js_name = manageStorage)]
        async fn manage_storage(operation: &str, q: u32) -> Result<JsValue, JsValue>;
    }
    let operation = operation.to_owned();
    spawn(async move {
        let result = match manage_storage(&operation, q.unwrap_or(0) as u32).await {
            Ok(v) => serde_json::from_str(&v.as_string().unwrap_or_default())
                .map_err(|_| "缓存信息无效".into()),
            Err(e) => Err(e.as_string().unwrap_or_else(|| "无法读取参数缓存".into())),
        };
        deliver(&queue, &ctx, FileEvent::Storage(result));
    });
}
