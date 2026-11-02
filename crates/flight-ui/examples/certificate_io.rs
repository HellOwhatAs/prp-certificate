use flight_ui::certificate::{self, Verified};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let item: Verified = serde_json::from_slice(&std::fs::read(&args[1]).unwrap()).unwrap();
    if args.get(3).is_some_and(|s| s == "--check") {
        let imported = certificate::parse_file(&std::fs::read(&args[2]).unwrap()).unwrap();
        assert_eq!(imported.certificate, item.certificate);
        certificate::check_document(&imported, &item).unwrap();
        println!("Carrier and decoded certificate match the independently verified result");
    } else {
        let bytes = certificate::export_png(&item).unwrap();
        std::fs::write(&args[2], &bytes).unwrap();
        println!("Certificate exported: {} bytes", bytes.len());
    }
}
