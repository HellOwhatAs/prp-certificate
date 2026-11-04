use flight_core::checker::{check, parse_csv};
use std::collections::BTreeMap;

const ROUTES_HEADER: &str = "aircraft_type,flight_no,stop_order,facility_id,refuel\n";
const ASSIGNMENTS_HEADER: &str =
    "person_id,aircraft_type,flight_no,pickup_stop_order,delivery_stop_order\n";
const Q3_ASSIGNMENTS_HEADER: &str =
    "person_id,aircraft_id,flight_no,pickup_stop_order,delivery_stop_order\n";
const SHORT_ROUTES: &str = concat!(
    "aircraft_type,flight_no,stop_order,facility_id,refuel\n",
    "T2,1,0,A01,0\nT2,1,1,F006,1\nT2,1,2,A01,0\n"
);

fn contains_all(error: &str, expected: &[&str]) {
    for part in expected {
        assert!(
            error.contains(part),
            "missing {part:?} in diagnostic: {error}"
        );
    }
}

fn route_error(routes: &str) -> String {
    check(
        1,
        routes,
        &format!("{ASSIGNMENTS_HEADER}P0002,T2,1,0,1\n"),
        false,
    )
    .err()
    .expect("expected an invalid route")
}

#[test]
fn csv_syntax_reports_physical_line_and_field_column() {
    for (csv, expected) in [
        (
            "a,b\r\n\r\n\"x\r\ny\"z,2",
            vec!["第 4 行第 1 列", "引号后的内容", "'z'"],
        ),
        (
            "a,b\n\n1,\"unfinished\n",
            vec!["第 3 行第 2 列", "引号未闭合"],
        ),
        ("a,b\n1x\"y,2", vec!["第 2 行第 1 列", "引号格式错误"]),
        ("a,b\n\n1,\0", vec!["第 3 行第 2 列", "NUL"]),
    ] {
        contains_all(&parse_csv(csv, &["a", "b"], 10).unwrap_err(), &expected);
    }
}

#[test]
fn csv_shape_errors_show_actual_and_expected_values() {
    contains_all(
        &parse_csv("a,c\n1,2", &["a", "b"], 10).unwrap_err(),
        &["第 1 行", "实际", "a,c", "应为", "a,b"],
    );
    contains_all(
        &parse_csv("a,b\n\n1,2,3", &["a", "b"], 10).unwrap_err(),
        &["第 3 行", "实际 3 列", "应为 2 列"],
    );
    contains_all(
        &parse_csv("a,b\n\"x\ny\",2,3", &["a", "b"], 10).unwrap_err(),
        &["第 2 行", "实际 3 列", "应为 2 列"],
    );
    contains_all(
        &parse_csv("a,b\n1,2\n\n3,4", &["a", "b"], 1).unwrap_err(),
        &["第 4 行", "超过上限 1"],
    );
}

#[test]
fn csv_context_tracking_preserves_accepted_fields() {
    assert_eq!(
        parse_csv(
            "\u{feff}a,b\r\n\r\n\"one\r\ntwo\",\"a\"\"b\"\r\n\"x,y\",z\r",
            &["a", "b"],
            2,
        )
        .unwrap(),
        vec![vec!["one\r\ntwo", "a\"b"], vec!["x,y", "z"]]
    );
}

#[test]
fn route_fields_and_duplicate_stops_keep_original_csv_lines() {
    contains_all(
        &route_error(&format!(
            "{ROUTES_HEADER}\nT2,1,0,A01,0\nT2,1,1,NOWHERE,0\n"
        )),
        &[
            "航线表第 4 行",
            "T2",
            "flight_no=1",
            "stop_order=1",
            "facility_id",
            "NOWHERE",
        ],
    );
    contains_all(
        &route_error(&format!("{ROUTES_HEADER}T9,1,0,A01,0\n")),
        &["航线表第 2 行", "aircraft_type", "T9", "未知机型或飞机"],
    );
    contains_all(
        &route_error(&format!("{ROUTES_HEADER}T2,01,0,A01,0\n")),
        &["航线表第 2 行", "flight_no", "01", "整数格式"],
    );
    contains_all(
        &route_error(&format!("{ROUTES_HEADER}T2,1,0,A01,0\n\nT2,1,0,A01,0\n")),
        &[
            "航线表第 4 行",
            "flight_no=1",
            "stop_order=0",
            "重复",
            "第 2 行",
        ],
    );
    contains_all(
        &route_error(&SHORT_ROUTES.replace(",1,1,F006,1", ",1,3,F006,1")),
        &["航线表第 4 行", "stop_order=2", "实际 2", "应为 1"],
    );
}

#[test]
fn insufficient_fuel_identifies_the_leg_and_quantities() {
    contains_all(
        &route_error(&SHORT_ROUTES.replace("F006,1", "F006,0")),
        &[
            "航线表第 4 行",
            "T2",
            "flight_no=1",
            "stop_order=1（F006）→2（A01）",
            "到达余油 -25.0 kg",
            "安全余油 150.0 kg",
            "距离 235 km",
            "耗油 587.5 kg",
        ],
    );
}

fn q3_routes(starts: &[i64]) -> String {
    let epoch = chrono::NaiveDate::from_ymd_opt(2026, 8, 3)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let time = |m| {
        (epoch + chrono::Duration::minutes(m))
            .format("%Y-%m-%d %H:%M")
            .to_string()
    };
    let mut csv = String::from(
        "aircraft_id,flight_no,stop_order,facility_id,arrival_time,departure_time,refuel\n",
    );
    for (i, start) in starts.iter().enumerate() {
        let no = i + 1;
        // Public A01–F006 distance is 235 km; T2 takes 65 minutes each way.
        csv += &format!("A01-T2-H01,{no},0,A01,,{},0\n", time(*start));
        csv += &format!(
            "A01-T2-H01,{no},1,F006,{},{},1\n",
            time(start + 65),
            time(start + 85)
        );
        csv += &format!("A01-T2-H01,{no},2,A01,{},,0\n", time(start + 150));
    }
    csv
}

fn q3_error(routes: &str, person: &str) -> String {
    check(
        3,
        routes,
        &format!("{Q3_ASSIGNMENTS_HEADER}{person},A01-T2-H01,1,0,1\n"),
        false,
    )
    .err()
    .expect("the small diagnostic example omits other personnel")
}

#[test]
fn q3_time_errors_identify_fields_stops_and_expected_times() {
    contains_all(
        &q3_error(
            &q3_routes(&[360]).replace("2026-08-03 06:00", "tomorrow"),
            "P0042",
        ),
        &[
            "航线表第 2 行",
            "A01-T2-H01",
            "departure_time",
            "tomorrow",
            "YYYY-MM-DD HH:MM",
        ],
    );
    contains_all(
        &q3_error(&q3_routes(&[360]).replace("07:05", "07:04"), "P0042"),
        &[
            "航线表第 3 行",
            "stop_order=1",
            "实际 2026-08-03 07:04",
            "应为 2026-08-03 07:05",
        ],
    );
    contains_all(
        &q3_error(&q3_routes(&[360]).replace("07:25", "07:24"), "P0042"),
        &[
            "航线表第 3 行",
            "stop_order=1",
            "实际 19 分钟",
            "至少需要 20 分钟",
        ],
    );
    contains_all(
        &q3_error(&q3_routes(&[360, 539]), "P0042"),
        &[
            "A01-T2-H01",
            "flight_no=2",
            "上一架次 flight_no=1",
            "实际间隔 29 分钟",
            "至少需要 30 分钟",
            "最早起飞 2026-08-03 09:00",
        ],
    );
    // The exact 20-minute refuel and 30-minute turnaround boundaries pass
    // route validation; the next error is the intentionally incomplete roster.
    contains_all(
        &q3_error(&q3_routes(&[360, 540]), "P0042"),
        &["人员分配表遗漏 3999 名人员"],
    );
}

#[test]
fn q3_person_time_window_shows_actual_and_required_times() {
    contains_all(
        &q3_error(&q3_routes(&[360]), "P0044"),
        &[
            "人员分配表第 2 行",
            "person_id=P0044",
            "A01-T2-H01",
            "flight_no=1",
            "stop_order=0",
            "2026-08-03 06:00",
            "最早可接人 2026-08-05 07:00",
            "最晚须送达",
        ],
    );
}

#[test]
fn assignment_errors_identify_the_person_and_reference() {
    for (record, expected) in [
        (
            "UNKNOWN,T2,1,0,1",
            vec!["person_id=\"UNKNOWN\"", "不属于第 1 问"],
        ),
        (
            "P0002,T9,1,0,1",
            vec!["person_id=P0002", "aircraft_type", "T9"],
        ),
        (
            "P0002,T2,2,0,1",
            vec!["person_id=P0002", "T2", "flight_no=2", "不存在的架次"],
        ),
        (
            "P0002,T2,1,0,9",
            vec!["pickup_stop_order=0", "delivery_stop_order=9", "≤ 2"],
        ),
        (
            "P0002,T2,1,0,1",
            vec![
                "person_id=P0002",
                "实际 stop_order=0（A01）→1（F006）",
                "需求为 任一机场（LAND）→F022",
            ],
        ),
    ] {
        let error = check(
            1,
            SHORT_ROUTES,
            &format!("{ASSIGNMENTS_HEADER}\n{record}\n"),
            false,
        )
        .err()
        .expect("expected an invalid assignment");
        contains_all(&error, &["人员分配表第 3 行"]);
        contains_all(&error, &expected);
    }
}

#[test]
fn capacity_error_reports_flight_leg_actual_load_and_seat_limit() {
    let routes = include_str!("../../../tests/fixtures/public/q1-routes.csv");
    let assignments = include_str!("../../../tests/fixtures/public/q1-assignments.csv");
    assert!(check(1, routes, assignments, false).is_ok());

    let route_rows = parse_csv(
        routes,
        &ROUTES_HEADER.trim().split(',').collect::<Vec<_>>(),
        40000,
    )
    .unwrap();
    let mut people = parse_csv(
        assignments,
        &ASSIGNMENTS_HEADER.trim().split(',').collect::<Vec<_>>(),
        1600,
    )
    .unwrap();
    let mut paths: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for row in route_rows {
        paths
            .entry(row[1].clone())
            .or_default()
            .push(row[2..].to_vec());
    }
    let mut loads: BTreeMap<String, usize> = BTreeMap::new();
    for row in &people {
        *loads.entry(row[2].clone()).or_default() += 1;
    }
    // Move one person between identical routes and pickup/delivery stops. All
    // individual assignments stay valid, while one full T3 flight gains a 20th
    // passenger. The fixture is deliberately outbound-only, with pickup at 0.
    let (source, target) = people
        .iter()
        .enumerate()
        .find_map(|(target, full)| {
            if loads[&full[2]] != 19 {
                return None;
            }
            people
                .iter()
                .position(|other| {
                    other[2] != full[2]
                        && other[1] == full[1]
                        && other[3..] == full[3..]
                        && paths[&other[2]] == paths[&full[2]]
                })
                .map(|source| (source, target))
        })
        .expect("public fixture contains a full T3 flight and another identical route");
    let target_fields = people[target][1..].to_vec();
    people[source][1..].clone_from_slice(&target_fields);
    let overloaded = format!(
        "{ASSIGNMENTS_HEADER}{}\n",
        people
            .iter()
            .map(|row| row.join(","))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let error = check(1, routes, &overloaded, false)
        .err()
        .expect("20 passengers exceed T3's 19 seats");
    contains_all(
        &error,
        &[
            "航线表第",
            "T3",
            "flight_no=",
            "stop_order=0",
            "飞往 stop_order=1",
            "载客 20 人",
            "19 个座位",
        ],
    );
}
