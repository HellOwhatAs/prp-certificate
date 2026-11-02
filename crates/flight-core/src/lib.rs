pub mod checker;
mod engine;
pub mod groth16;
mod parameters;
pub use engine::{run_job, verify_certificate};

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_stdio() {
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    fn emit(v: Value) {
        println!("{v}");
        let _ = std::io::stdout().flush();
    }
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        let mut input = String::new();
        std::io::stdin()
            .take(42_000_001)
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 42_000_000 {
            return Err("输入过大".into());
        }
        let job: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        let circuit = vec![];
        let mut g1 = vec![];
        if job["action"] == "prove" {
            let checked = run_job(
                json!({"action":"check","q":job["q"],"files":job["files"]}),
                vec![],
                vec![],
                |_| {},
            )?;
            if checked["canProve"] != true {
                return Err("当前证明上限为 256 架次".into());
            }
            let q = job["q"]
                .as_u64()
                .filter(|n| (1..=3).contains(n))
                .ok_or("题号无效")?;
            let mut candidates = vec![];
            if let Some(p) = std::env::var_os("FLIGHT_HOME") {
                candidates.push(std::path::PathBuf::from(p));
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(p) = exe.parent() {
                    candidates.extend(p.ancestors().take(5).map(|p| p.to_owned()));
                }
            }
            if let Ok(cwd) = std::env::current_dir() {
                candidates.push(cwd);
            }
            let root = candidates
                .into_iter()
                .find(|p| p.join("public/groth16/manifest.json").is_file())
                .ok_or("未找到公开证明参数，请使用完整发行包")?;
            let info = &engine::manifest()["circuits"][q.to_string()];
            g1.reserve(info["pk_bytes"].as_u64().ok_or("参数长度无效")? as usize);
            for chunk in info["chunks"].as_array().ok_or("参数清单无效")? {
                let bytes = std::fs::read(
                    root.join("public/groth16")
                        .join(chunk["file"].as_str().ok_or("参数文件无效")?),
                )
                .map_err(|_| format!("缺少问题 {q} 的公开参数，请将对应参数包解压到程序同目录"))?;
                if bytes.len() as u64 != chunk["bytes"].as_u64().ok_or("参数长度无效")? {
                    return Err("公开参数长度无效".into());
                }
                g1.extend(bytes);
            }
        }
        run_job(job, circuit, g1, |message| {
            emit(json!({"type":"progress","text":message}))
        })
    });
    match result {
        Ok(Ok(value)) => emit(json!({"type":"result","result":value})),
        Ok(Err(message)) => emit(json!({"type":"error","message":message})),
        Err(_) => emit(json!({"type":"error","message":"证明数据无效或计算引擎异常"})),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn execute(
    job_json: &str,
    circuit: Vec<u8>,
    g1: Vec<u8>,
    progress: js_sys::Function,
) -> Result<String, wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    let result = (|| {
        if job_json.len() > 42_000_000 {
            return Err("输入过大".to_owned());
        }
        let job = serde_json::from_str(job_json).map_err(|e| format!("输入格式无效：{e}"))?;
        let result = run_job(job, circuit, g1, |message| {
            let _ = progress.call1(
                &wasm_bindgen::JsValue::NULL,
                &wasm_bindgen::JsValue::from_str(message),
            );
        })?;
        serde_json::to_string(&result).map_err(|e| e.to_string())
    })();
    result.map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}
