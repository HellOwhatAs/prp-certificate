mod app;
mod carrier;
pub mod certificate;
mod materials;
mod platform;
pub use app::FlightApp;

// Image decoding and verification run in a disposable worker / child process.
#[cfg(not(target_arch = "wasm32"))]
pub fn import_stdio() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::{Value, json};
    use std::io::Read;
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        let mut input = String::new();
        std::io::stdin()
            .take(22_000_001)
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 22_000_000 {
            return Err("证书文件过大".into());
        }
        let job: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        let bytes = STANDARD
            .decode(job["file"].as_str().ok_or("缺少证书文件")?)
            .map_err(|_| "证书编码无效")?;
        println!("{}", json!({"type":"progress","text":"读取证书与证明…"}));
        let imported = certificate::parse_file(&bytes)?;
        let result = flight_core::verify_certificate(imported.certificate.clone(), |message| {
            println!("{}", json!({"type":"progress","text":message}))
        })?;
        let verified = serde_json::from_value(result.clone()).map_err(|e| e.to_string())?;
        certificate::check_document(&imported, &verified)?;
        Ok(result)
    });
    let output = match result {
        Ok(Ok(result)) => json!({"type":"result","result":result}),
        Ok(Err(message)) => json!({"type":"error","message":message}),
        Err(_) => json!({"type":"error","message":"证书读取失败"}),
    };
    println!("{output}");
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn parse_certificate_file(bytes: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    let imported =
        certificate::parse_file(bytes).map_err(|e| wasm_bindgen::JsValue::from_str(&e))?;
    Ok(imported.certificate.to_string())
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn certificate_png(item_json: &str) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let item = serde_json::from_str(item_json)
        .map_err(|e| wasm_bindgen::JsValue::from_str(&format!("{e}")))?;
    certificate::export_png(&item).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| Ok(Box::new(FlightApp::new(cc)))),
        )
        .await
}
