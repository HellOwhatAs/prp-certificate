fn main() {
    for key in ["FLIGHT_PARAMETER_REPOSITORY", "FLIGHT_PARAMETER_REF"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let repository = std::env::var("FLIGHT_PARAMETER_REPOSITORY")
        .unwrap_or_else(|_| "HellOwhatAs/prp-certificate".into());
    let reference = std::env::var("FLIGHT_PARAMETER_REF").unwrap_or_else(|_| "main".into());
    let safe = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            && s != "."
            && s != ".."
    };
    let pieces: Vec<_> = repository.split('/').collect();
    assert!(
        pieces.len() == 2 && pieces.iter().all(|s| safe(s)),
        "invalid parameter repository"
    );
    assert!(reference.split('/').all(safe), "invalid parameter ref");
    println!(
        "cargo:rustc-env=FLIGHT_PARAMETER_BASE_URL=https://raw.githubusercontent.com/{repository}/{reference}/public/groth16/"
    );
}
