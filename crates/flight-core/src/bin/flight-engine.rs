#[cfg(not(target_arch = "wasm32"))]
fn main() {
    flight_core::run_native_stdio();
}
#[cfg(target_arch = "wasm32")]
fn main() {}
