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

/// Uses private organizer fixtures only when explicitly supplied; never bundles them.
#[test]
fn private_reference_regression() {
    let Ok(dir) = std::env::var("REFERENCE_DIR") else {
        return;
    };
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
