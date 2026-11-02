use flight_core::{
    checker,
    groth16::circuit::{self, Witness},
};
fn main() {
    let path = std::env::args().nth(1).expect("private job path");
    let job: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let q = job["q"].as_u64().unwrap() as usize;
    let checked = checker::check(
        q,
        job["files"]["routes"].as_str().unwrap(),
        job["files"]["assignments"].as_str().unwrap(),
        false,
    )
    .unwrap();
    let w: Witness = serde_json::from_value(checked.groth_witness().unwrap()).unwrap();
    let base = circuit::compile(q, &[17; 32], &w).unwrap();
    base.satisfied().unwrap();
    let digest = base.digest();
    drop(base);
    let blank = circuit::compile(q, &[0; 32], &Witness::blank(q)).unwrap();
    assert_eq!(digest, blank.digest());
    drop(blank);
    for case in [
        "score",
        "omit-person",
        "load",
        "pickup-delivery",
        "location",
        "resource",
        "length",
    ] {
        let mut altered = w.clone();
        match case {
            "score" => altered.score[0] += 1,
            "omit-person" => altered.served[0] = false,
            "load" => altered.loads[0] += 1,
            "pickup-delivery" => altered.delivery[0] = altered.pickup[0],
            "location" => altered.loc[0] = 55,
            "resource" => altered.resources[0] = 24,
            "length" => altered.lengths[0] = 2,
            _ => unreachable!(),
        }
        let b = circuit::compile(q, &[17; 32], &altered).unwrap();
        assert_eq!(digest, b.digest(), "{case}: witness-dependent constraints");
        assert!(b.satisfied().is_err(), "{case}: accepted invalid witness");
        eprintln!("Q{q}: {case} rejected");
    }
    if q == 1 {
        let pair = (0..256)
            .find_map(|a| {
                (a + 1..256)
                    .find(|b| {
                        w.lengths[a] > 0 && w.lengths[*b] > 0 && w.resources[a] == w.resources[*b]
                    })
                    .map(|b| (a, b))
            })
            .unwrap();
        let (a, b) = pair;
        let mut other = w.clone();
        other.resources.swap(a, b);
        other.lengths.swap(a, b);
        for k in 0..7 {
            other.loc.swap(a * 7 + k, b * 7 + k);
            other.refuel.swap(a * 7 + k, b * 7 + k);
            other.arrivals.swap(a * 7 + k, b * 7 + k);
            other.departures.swap(a * 7 + k, b * 7 + k);
            other.loads.swap(a * 7 + k, b * 7 + k);
        }
        for f in &mut other.flight {
            if *f == a as u64 {
                *f = b as u64;
            } else if *f == b as u64 {
                *f = a as u64;
            }
        }
        let cs = circuit::compile(q, &[91; 32], &other).unwrap();
        assert_eq!(digest, cs.digest());
        cs.satisfied().unwrap();
        eprintln!("Q1: distinct valid route ordering and identity preserve setup matrix");
    }
    eprintln!(
        "Q{q}: original claims {}, all adversarial regressions passed",
        serde_json::to_string(&w.score).unwrap()
    );
}
