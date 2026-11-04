use flight_core::checker::{check, parse_csv};

#[test]
fn csv_rejects_ambiguous_or_unbounded_input() {
    let header = ["a", "b"];
    for value in [
        "a,b\n1,2,3",
        "a,b\n\"unfinished",
        "a,b\n\"1\"x,2",
        "a,b\n1\0,2",
        "a,b\n1,2\n3,4",
    ] {
        assert!(parse_csv(value, &header, 1).is_err(), "{value:?}");
    }
    assert_eq!(
        parse_csv("\u{feff}a,b\r\n\"1\",\"2\"\r\n", &header, 1).unwrap(),
        vec![vec!["1", "2"]]
    );
    assert_eq!(
        parse_csv("a,b\n\"a,b\",\"a\"\"b\"", &header, 1).unwrap(),
        vec![vec!["a,b", "a\"b"]]
    );
}

#[test]
fn public_q1_reference_regression() {
    let routes = include_str!("../../../tests/fixtures/public/q1-routes.csv");
    let assignments = include_str!("../../../tests/fixtures/public/q1-assignments.csv");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/public/q1-expected.json"
    ))
    .unwrap();
    let checked = check(1, routes, assignments, false).unwrap();
    assert_eq!(serde_json::json!(checked.score), expected["scores"]);
    assert!(checked.can_prove());

    let rows: Vec<_> = assignments.trim().lines().collect();
    assert!(check(1, routes, &rows[..rows.len() - 1].join("\n"), false).is_err());
    assert!(
        check(
            1,
            routes,
            &format!("{}\n{}", assignments.trim(), rows[1]),
            false
        )
        .is_err()
    );
    assert!(
        check(
            1,
            routes,
            &assignments.replacen("P0002", "UNKNOWN", 1),
            false
        )
        .is_err()
    );
    assert!(
        check(
            1,
            &routes.replacen(",1,0,", ",1.5,0,", 1),
            assignments,
            false
        )
        .is_err()
    );
    assert!(check(1, &routes.replacen(",A01,", ",A02,", 1), assignments, false).is_err());
}

#[test]
fn public_certificate_uses_the_fixed_verification_key() {
    use serde_json::{Value, json};
    let original: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/certificates/public-q1.json"
    ))
    .unwrap();
    let certificate = &original["certificate"];
    let verified = flight_core::verify_certificate(certificate.clone(), |_| {}).unwrap();
    assert_eq!(verified, original);
    let expected: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/public/q1-expected.json"
    ))
    .unwrap();
    assert_eq!(verified["scores"][0], expected["scores"]);

    for score in 0..9 {
        let mut changed = certificate.clone();
        let value = changed["certificates"][0]["scores"][score]
            .as_u64()
            .unwrap();
        changed["certificates"][0]["scores"][score] = json!(value + 1);
        assert!(
            flight_core::verify_certificate(changed, |_| {}).is_err(),
            "score {score}"
        );
    }
    for (field, value) in [
        ("team", json!("Forged team")),
        ("nonce", json!("00000000-0000-4000-8000-000000000000")),
        ("question", json!(2)),
        ("dataset", json!("untrusted dataset")),
        ("rules", json!("untrusted rules")),
    ] {
        let mut changed = certificate.clone();
        changed["certificates"][0]["metadata"][field] = value;
        assert!(
            flight_core::verify_certificate(changed, |_| {}).is_err(),
            "metadata {field}"
        );
    }
    let mut changed = certificate.clone();
    changed["certificates"][0]["vk"] = json!("certificate-supplied keys must never be trusted");
    assert!(flight_core::verify_certificate(changed, |_| {}).is_err());
    let mut changed = certificate.clone();
    let mut second = certificate["certificates"][0].clone();
    second["metadata"]["question"] = json!(2);
    changed["certificates"].as_array_mut().unwrap().push(second);
    assert!(flight_core::verify_certificate(changed, |_| {}).is_err());
}

/// Private organizer fixtures are deliberately absent from the public source.
#[test]
#[ignore = "requires REFERENCE_DIR with private Q1/Q2/Q3 organizer fixtures"]
fn private_reference_regression() {
    let dir = std::env::var("REFERENCE_DIR")
        .expect("set REFERENCE_DIR before explicitly running private_reference_regression");
    for q in 1..=3 {
        let routes = std::fs::read_to_string(format!("{dir}/q{q}-routes.csv")).unwrap();
        let assignments = std::fs::read_to_string(format!("{dir}/q{q}-assignments.csv")).unwrap();
        let checked = check(q, &routes, &assignments, false).unwrap();
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(format!("{dir}/reports/metrics_q{q}.json")).unwrap(),
        )
        .unwrap();
        let m = &report["metrics"];
        for (i, key) in [
            (0, "aircraft_minutes"),
            (1, "person_minutes"),
            (3, "flight_count"),
            (4, "landings"),
            (5, "passenger_km"),
            (6, "available_seat_km"),
            (8, "empty_leg_km"),
        ] {
            assert_eq!(
                checked.score[i] as f64,
                m[key].as_f64().unwrap(),
                "Q{q} {key}"
            );
        }
        assert_eq!(
            checked.score[2] as f64 / 10.,
            m["fuel_kg"].as_f64().unwrap()
        );
        let rows: Vec<_> = assignments.trim().lines().collect();
        assert!(check(q, &routes, &rows[..rows.len() - 1].join("\n"), false).is_err());
        assert!(
            check(
                q,
                &routes,
                &format!("{}\n{}", assignments.trim(), rows[1]),
                false
            )
            .is_err()
        );
        let first_id = rows[1].split(',').next().unwrap();
        assert!(
            check(
                q,
                &routes,
                &assignments.replacen(first_id, "UNKNOWN", 1),
                false
            )
            .is_err()
        );
        let bad_routes = routes.replacen(",1,0,", ",1.5,0,", 1);
        assert_ne!(bad_routes, routes);
        assert!(check(q, &bad_routes, &assignments, false).is_err());
        if q == 3 {
            assert!(check(q, &routes, &assignments, true).is_err());
        }
    }
}
