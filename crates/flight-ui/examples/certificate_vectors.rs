use flight_ui::certificate::{self, Verified};
fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&dir).unwrap();
    let parts: Vec<_> = (1..=3)
        .map(|q| {
            use base64::Engine;
            let score = [
                1234567, 98765432, 100019, 255, 987, 12345678, 34567890, 157, 789,
            ];
            Verified {
                certificate: serde_json::to_value(
                    flight_format::CertificateSet::new(vec![flight_format::Certificate {
                        metadata: flight_format::Metadata::new(
                            q,
                            "渲染测试 Abc0123 海上运输".into(),
                            flight_format::nonce(&[q as u8; 16]),
                        ),
                        proof: base64::engine::general_purpose::STANDARD.encode([42; 128]),
                        scores: score,
                    }])
                    .unwrap(),
                )
                .unwrap(),
                scores: vec![score.iter().map(|x| *x as u64).collect()],
            }
        })
        .collect();
    for mask in 1..=7 {
        let selected: Vec<_> = parts
            .iter()
            .enumerate()
            .filter(|(q, _)| mask & (1 << q) != 0)
            .map(|(_, p)| p.clone())
            .collect();
        let item = Verified::combine(&selected).unwrap();
        write(&dir, &format!("set-{mask}"), &item);
    }
    for part in &parts {
        write(&dir, &format!("q{}", part.question()), part);
    }
    let mut long = parts.clone();
    for p in &mut long {
        p.certificate["certificates"][0]["metadata"]["team"] =
            serde_json::json!("长队名测试".repeat(8));
        p.certificate["certificates"][0]["scores"] = serde_json::to_value([u32::MAX; 9]).unwrap();
        p.scores = vec![vec![u32::MAX as u64; 9]];
    }
    write(&dir, "long", &Verified::combine(&long).unwrap());
}
fn write(dir: &str, name: &str, item: &Verified) {
    // Synthetic codec fixture; deliberately not a valid cryptographic certificate.
    std::fs::write(
        format!("{dir}/{name}.json"),
        serde_json::to_vec(item).unwrap(),
    )
    .unwrap();
    std::fs::write(
        format!("{dir}/{name}.png"),
        certificate::export_png(item).unwrap(),
    )
    .unwrap();
}
