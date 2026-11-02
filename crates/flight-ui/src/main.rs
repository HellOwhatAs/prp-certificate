#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    if std::env::args().any(|arg| arg == "--engine") {
        flight_core::run_native_stdio();
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--import") {
        flight_ui::import_stdio();
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            // Keep the existing storage identity when the display title changes.
            .with_app_id("海上油田人员直升机运载计划编排")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([720.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "海上油田人员直升机运载计划编排",
        options,
        Box::new(|cc| Ok(Box::new(flight_ui::FlightApp::new(cc)))),
    )
}
#[cfg(target_arch = "wasm32")]
fn main() {}
