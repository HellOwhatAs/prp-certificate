use flight_core::{
    checker,
    groth16::circuit::{self, Witness},
};
fn main() {
    let path = std::env::args().nth(1).expect("job path");
    let job: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let q = job["q"].as_u64().unwrap() as usize;
    let total = std::time::Instant::now();
    let c = checker::check(
        q,
        job["files"]["routes"].as_str().unwrap(),
        job["files"]["assignments"].as_str().unwrap(),
        false,
    )
    .unwrap();
    eprintln!("check {:.3}s", total.elapsed().as_secs_f64());
    let start = std::time::Instant::now();
    let w: Witness = serde_json::from_value(c.groth_witness().unwrap()).unwrap();
    eprintln!("witness {:.3}s", start.elapsed().as_secs_f64());
    let start = std::time::Instant::now();
    let b = circuit::compile(q, &[17; 32], &w).unwrap();
    eprintln!(
        "Q{q}: {} build {:.2}s",
        b.stats(),
        start.elapsed().as_secs_f64()
    );
    let start = std::time::Instant::now();
    b.satisfied().unwrap();
    eprintln!("satisfied {:.3}s", start.elapsed().as_secs_f64());
    let start = std::time::Instant::now();
    let digest = b.digest();
    eprintln!(
        "Q{q}: digest {digest} {:.3}s",
        start.elapsed().as_secs_f64()
    );
    drop(b);
    let b = circuit::compile(q, &[0; 32], &Witness::blank(q)).unwrap();
    assert_eq!(digest, b.digest(), "witness-dependent circuit shape");
    eprintln!("Q{q}: blank setup has identical matrix digest");
    drop(b);
    if std::env::args().any(|arg| arg == "--prove") {
        // Local public parameters only; no answer/certificate is written.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../public/groth16");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
        let mut params = Vec::new();
        for chunk in manifest["circuits"][q.to_string()]["chunks"]
            .as_array()
            .unwrap()
        {
            params.extend(std::fs::read(root.join(chunk["file"].as_str().unwrap())).unwrap());
        }
        let mut job = job;
        job["action"] = "prove".into();
        let start = std::time::Instant::now();
        let mut last = start;
        let result = flight_core::run_job(job, vec![], params, |message| {
            let now = std::time::Instant::now();
            eprintln!(
                "{:.3}s (+{:.3}s) {message}",
                now.duration_since(start).as_secs_f64(),
                now.duration_since(last).as_secs_f64()
            );
            last = now;
        })
        .unwrap();
        assert_eq!(result["scores"][0], serde_json::json!(c.score));
        eprintln!(
            "Q{q}: full proof + verification {:.3}s",
            start.elapsed().as_secs_f64()
        );
    }
}
