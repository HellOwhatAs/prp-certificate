use flight_ui::certificate::{self, Verified};
fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&dir).unwrap();
    for q in 1..=3 {
        let score = vec![
            1234567, 98765432, 100019, 255, 987, 12345678, 34567890, 157, 789,
        ];
        let item = Verified {
            certificate: {
                use base64::Engine;
                serde_json::to_value(flight_format::Certificate {
                    metadata: flight_format::Metadata::new(
                        q,
                        if q == 3 {
                            "长队名测试".repeat(8)
                        } else {
                            "渲染测试 Abc0123 海上运输".into()
                        },
                        "12345678-1234-1234-1234-123456789012".into(),
                    ),
                    proof: base64::engine::general_purpose::STANDARD.encode([42; 128]),
                    scores: score.clone().try_into().unwrap(),
                })
                .unwrap()
            },
            scores: vec![score.into_iter().map(|x| x as u64).collect()],
        };
        // Synthetic codec fixture, deliberately not a valid cryptographic certificate.
        std::fs::write(
            format!("{dir}/q{q}.json"),
            serde_json::to_vec(&item).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/q{q}.png"),
            certificate::export_png(&item).unwrap(),
        )
        .unwrap();
    }
}
