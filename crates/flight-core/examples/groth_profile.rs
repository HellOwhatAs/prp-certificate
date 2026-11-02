use flight_core::{
    checker,
    groth16::circuit::{self, Witness},
};
fn main() {
    let path = std::env::args().nth(1).expect("job path");
    let job: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let q = job["q"].as_u64().unwrap() as usize;
    let c = checker::check(
        q,
        job["files"]["routes"].as_str().unwrap(),
        job["files"]["assignments"].as_str().unwrap(),
        false,
    )
    .unwrap();
    let w: Witness = serde_json::from_value(c.groth_witness().unwrap()).unwrap();
    let start = std::time::Instant::now();
    let b = circuit::compile(q, &[17; 32], &w).unwrap();
    eprintln!(
        "Q{q}: {} build {:.2}s",
        b.stats(),
        start.elapsed().as_secs_f64()
    );
    b.satisfied().unwrap();
    let digest = b.digest();
    eprintln!(
        "Q{q}: satisfied digest {digest} {:.2}s",
        start.elapsed().as_secs_f64()
    );
    drop(b);
    let b = circuit::compile(q, &[0; 32], &Witness::blank(q)).unwrap();
    assert_eq!(digest, b.digest(), "witness-dependent circuit shape");
    eprintln!("Q{q}: blank setup has identical matrix digest");
}
