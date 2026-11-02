use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

pub const PROFILE: usize = 256;
macro_rules! ensure { ($cond:expr,$($msg:tt)*) => {if !($cond) {return Err(format!($($msg)*));}}; }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub dataset_id: String,
    pub(crate) locations: Vec<String>,
    pub(crate) planes: Vec<String>,
    pub(crate) plane_home: Vec<usize>,
    pub(crate) plane_type: Vec<usize>,
    pub(crate) capacity: Vec<u64>,
    pub(crate) speed: Vec<u64>,
    pub(crate) tank10: Vec<i64>,
    pub(crate) reserve10: Vec<i64>,
    pub(crate) fuel_rate10: Vec<u64>,
    pub(crate) refuel: Vec<usize>,
    pub(crate) distance: Vec<Vec<u64>>,
    pub(crate) people: HashMap<String, Vec<Person>>,
}
#[derive(Deserialize)]
pub(crate) struct Person {
    pub(crate) id: String,
    pub(crate) origin: usize,
    pub(crate) destination: usize,
    pub(crate) earliest: i64,
    pub(crate) latest: i64,
    pub(crate) task: String,
}
pub fn instance() -> &'static Instance {
    static DATA: OnceLock<Instance> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_slice(include_bytes!("../../../public/data/instance.json"))
            .expect("bundled instance")
    })
}
#[derive(Default, Clone)]
struct Stop {
    k: usize,
    loc: usize,
    refuel: bool,
    arr: Option<i64>,
    dep: Option<i64>,
}
#[derive(Default)]
struct Flight {
    resource: usize,
    no: usize,
    stops: Vec<Stop>,
    plane_type: usize,
    board: Vec<i64>,
    alight: Vec<i64>,
}
#[derive(Default, Clone)]
struct Assignment {
    served: bool,
    flight: usize,
    pickup: usize,
    delivery: usize,
}
pub struct Checked {
    pub score: Vec<u64>,
    flights: Vec<Flight>,
    assignments: Vec<Assignment>,
    q: usize,
    baseline: bool,
}
impl Checked {
    pub fn groth_witness(&self) -> Result<Value, String> {
        let mut out = self.witness(&[0; 32])?;
        let mut arr = vec![0u64; PROFILE * 7];
        let mut dep = vec![0u64; PROFILE * 7];
        let mut loads = vec![0u64; PROFILE * 7];
        for (f, r) in self.flights.iter().enumerate() {
            let mut load = 0i64;
            for (k, s) in r.stops.iter().enumerate() {
                arr[f * 7 + k] = s.arr.unwrap_or(0) as u64;
                dep[f * 7 + k] = if k + 1 == r.stops.len() {
                    0
                } else {
                    s.dep.unwrap_or(0) as u64
                };
                load += r.board[k] - r.alight[k];
                loads[f * 7 + k] = load as u64;
            }
        }
        out["arrivals"] = json!(arr);
        out["departures"] = json!(dep);
        out["loads"] = json!(loads);
        Ok(out)
    }
    pub fn can_prove(&self) -> bool {
        self.flights.len() <= PROFILE
    }
    pub fn witness(&self, identity: &[u8; 32]) -> Result<Value, String> {
        ensure!(self.can_prove(), "当前证明上限为 {PROFILE} 架次");
        let mut resources = vec![0; PROFILE];
        let mut lengths = vec![0; PROFILE];
        let mut loc = vec![0; PROFILE * 7];
        let mut refuel = vec![false; PROFILE * 7];
        let mut times_in = vec![0; PROFILE * 7];
        let mut times_out = vec![0; PROFILE * 7];
        for (f, r) in self.flights.iter().enumerate() {
            resources[f] = r.resource;
            lengths[f] = r.stops.len();
            for (k, s) in r.stops.iter().enumerate() {
                let i = f * 7 + k;
                loc[i] = s.loc;
                refuel[i] = s.refuel;
                if self.q == 3 {
                    times_in[i] = s.arr.unwrap_or(0);
                    times_out[i] = s.dep.unwrap_or(0);
                }
            }
        }
        Ok(
            json!({"identity":identity,"mode":if self.baseline {0}else{1},"score":self.score,
          "resources":resources,"lengths":lengths,"loc":loc,"refuel":refuel,"times_in":times_in,"times_out":times_out,
          "served":self.assignments.iter().map(|a|a.served).collect::<Vec<_>>(),
          "flight":self.assignments.iter().map(|a|a.flight).collect::<Vec<_>>(),
          "pickup":self.assignments.iter().map(|a|a.pickup).collect::<Vec<_>>(),
          "delivery":self.assignments.iter().map(|a|a.delivery).collect::<Vec<_>>() }),
        )
    }
}
pub fn parse_csv(text: &str, header: &[&str], max_rows: usize) -> Result<Vec<Vec<String>>, String> {
    ensure!(
        text.len() <= 10_000_000 && !text.contains('\0'),
        "CSV 含 NUL 或超过 10 MB"
    );
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            ensure!(field.is_empty() && !closed, "CSV 引号格式错误");
            quoted = true;
        } else if c == ',' || c == '\n' || c == '\r' {
            row.push(std::mem::take(&mut field));
            closed = false;
            if c != ',' {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if !(row.len() == 1 && row[0].is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                ensure!(rows.len() <= max_rows + 1, "CSV 数据行过多");
            }
        } else {
            ensure!(!closed, "CSV 引号后的内容无效");
            field.push(c);
        }
    }
    ensure!(!quoted, "CSV 引号未闭合");
    if !field.is_empty() || !row.is_empty() || closed {
        row.push(field);
        rows.push(row);
    }
    ensure!(
        !rows.is_empty()
            && rows[0]
                .iter()
                .map(String::as_str)
                .eq(header.iter().copied()),
        "表头必须为：{}",
        header.join(",")
    );
    rows.remove(0);
    ensure!(rows.len() <= max_rows, "CSV 数据行过多");
    ensure!(
        rows.iter().all(|r| r.len() == header.len()),
        "CSV 数据列数与表头不一致"
    );
    Ok(rows)
}
fn number(s: &str, min: usize) -> Result<usize, String> {
    ensure!(
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')),
        "整数格式无效：{s}"
    );
    let n = s
        .parse::<usize>()
        .map_err(|_| format!("整数超出范围：{s}"))?;
    ensure!(n >= min && n <= u32::MAX as usize, "整数超出范围：{s}");
    Ok(n)
}
fn minute(s: &str) -> Result<Option<i64>, String> {
    if s.is_empty() {
        return Ok(None);
    }
    let d = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M")
        .map_err(|_| "时间格式必须为 YYYY-MM-DD HH:MM")?;
    ensure!(
        d.format("%Y-%m-%d %H:%M").to_string() == s,
        "日期时间格式无效"
    );
    let base = chrono::NaiveDate::from_ymd_opt(2026, 8, 3)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let n = (d - base).num_minutes();
    ensure!((0..=10080).contains(&n), "时刻超出规划期");
    Ok(Some(n))
}
fn matches(endpoint: usize, location: usize) -> bool {
    if endpoint == 55 {
        location < 3
    } else {
        endpoint == location
    }
}
pub fn check(q: usize, routes: &str, assignments: &str, baseline: bool) -> Result<Checked, String> {
    ensure!((1..=3).contains(&q), "不支持的题号");
    let d = instance();
    let people = &d.people[&q.to_string()];
    let rf = if q == 3 {
        "aircraft_id"
    } else {
        "aircraft_type"
    };
    let rh = if q == 3 {
        vec![
            rf,
            "flight_no",
            "stop_order",
            "facility_id",
            "arrival_time",
            "departure_time",
            "refuel",
        ]
    } else {
        vec![rf, "flight_no", "stop_order", "facility_id", "refuel"]
    };
    let rr = parse_csv(routes, &rh, 40000).map_err(|e| format!("航线表：{e}"))?;
    let aa = parse_csv(
        assignments,
        &[
            "person_id",
            rf,
            "flight_no",
            "pickup_stop_order",
            "delivery_stop_order",
        ],
        people.len(),
    )
    .map_err(|e| format!("人员分配表：{e}"))?;
    ensure!(
        !rr.is_empty() && !aa.is_empty(),
        "航线表和人员分配表都不能为空"
    );
    let types = vec!["T1".to_owned(), "T2".into(), "T3".into()];
    let resources = if q == 3 { &d.planes } else { &types };
    let mut map: BTreeMap<(usize, usize), Flight> = BTreeMap::new();
    for (i, r) in rr.iter().enumerate() {
        let resource = resources
            .iter()
            .position(|x| x == &r[0])
            .ok_or_else(|| format!("航线表第 {} 行：未知机型或飞机", i + 2))?;
        let no = number(&r[1], 1)?;
        let k = number(&r[2], 0)?;
        let loc = d
            .locations
            .iter()
            .position(|x| x == &r[3])
            .ok_or("未知地点")?;
        let fuel = r.last().unwrap();
        ensure!(fuel == "0" || fuel == "1", "refuel 必须为 0 或 1");
        let refuel = fuel == "1";
        ensure!(!refuel || d.refuel.contains(&loc), "该地点不允许加油");
        let route = map.entry((resource, no)).or_insert_with(|| Flight {
            resource,
            no,
            ..Default::default()
        });
        ensure!(!route.stops.iter().any(|s| s.k == k), "停靠序号重复");
        route.stops.push(Stop {
            k,
            loc,
            refuel,
            arr: if q == 3 { minute(&r[4])? } else { None },
            dep: if q == 3 { minute(&r[5])? } else { None },
        });
    }
    let mut flights: Vec<_> = map.into_values().collect();
    let index: HashMap<_, _> = flights
        .iter()
        .enumerate()
        .map(|(i, r)| ((r.resource, r.no), i))
        .collect();
    let mut counts = vec![0; resources.len()];
    let mut last_end = vec![None; resources.len()];
    let mut score = vec![0u64; 9];
    score[3] = flights.len() as u64;
    for route in &mut flights {
        let r = route.resource;
        counts[r] += 1;
        ensure!(route.no == counts[r], "flight_no 必须从 1 连续编号");
        route.stops.sort_by_key(|s| s.k);
        let n = route.stops.len();
        ensure!((3..=7).contains(&n), "每架次须有 1–5 次海上停靠");
        ensure!(
            route.stops.iter().enumerate().all(|(i, s)| i == s.k),
            "stop_order 必须从 0 连续编号"
        );
        let base = route.stops[0].loc;
        ensure!(
            base < 3 && route.stops[n - 1].loc == base,
            "必须从同一机场出发并返回"
        );
        ensure!(q != 3 || base == d.plane_home[r], "必须从所属机场出发");
        ensure!(
            route.stops[1..n - 1].iter().all(|s| s.loc >= 3),
            "中途停靠必须为海上设施"
        );
        ensure!(
            !route.stops[0].refuel && !route.stops[n - 1].refuel,
            "机场停靠不能标记加油"
        );
        let t = if q == 3 { d.plane_type[r] } else { r };
        route.plane_type = t;
        let mut remaining = d.tank10[t];
        route.board = vec![0; n];
        route.alight = vec![0; n];
        for k in 0..n {
            let s = route.stops[k].clone();
            let last = k == n - 1;
            if q == 3 {
                ensure!(
                    if k == 0 {
                        s.arr.is_none() && s.dep.is_some()
                    } else if last {
                        s.arr.is_some() && s.dep.is_none()
                    } else {
                        s.arr.is_some() && s.dep.is_some()
                    },
                    "到达/离开时刻填写不完整（首到达、末离开留空）"
                );
            } else if k == 0 {
                route.stops[k].arr = Some(0);
                route.stops[k].dep = Some(0);
            }
            if k > 0 {
                let prev = &route.stops[k - 1];
                ensure!(prev.loc != s.loc, "不能连续停靠同一地点");
                let distance = d.distance[prev.loc][s.loc];
                let fuel = distance * d.fuel_rate10[t];
                remaining -= fuel as i64;
                score[2] += fuel;
                ensure!(remaining >= d.reserve10[t], "到达余油低于安全余油");
                if s.refuel {
                    remaining = d.tank10[t];
                }
                let expected =
                    prev.dep.ok_or("缺少出发时刻")? + (60 * distance).div_ceil(d.speed[t]) as i64;
                if q == 3 {
                    ensure!(
                        s.arr == Some(expected),
                        "到达时刻与航程不一致，应为规划期第 {expected} 分钟"
                    );
                } else {
                    route.stops[k].arr = Some(expected);
                    route.stops[k].dep = Some(
                        expected
                            + if last {
                                0
                            } else if s.refuel {
                                20
                            } else {
                                10
                            },
                    );
                }
                if !last {
                    ensure!(
                        route.stops[k].dep.unwrap() - route.stops[k].arr.unwrap()
                            >= if s.refuel { 20 } else { 10 },
                        "中途停留时间不足"
                    );
                }
            }
        }
        let start = route.stops[0].dep.unwrap();
        let end = route.stops[n - 1].arr.unwrap();
        if q == 3 {
            ensure!(
                (360..=1080).contains(&(start % 1440)),
                "须在 06:00–18:00 起飞"
            );
            ensure!(
                start / 1440 == end / 1440 && end % 1440 <= 1200,
                "须在当天 20:00 前返回"
            );
            ensure!(
                last_end[r].is_none_or(|prev| start >= prev + 30),
                "架次冲突或机场周转不足 30 分钟"
            );
            last_end[r] = Some(end);
        }
        score[0] += (end - start) as u64;
        score[4] += (n - 2) as u64;
    }
    let ids: HashMap<_, _> = people
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();
    let mut seen = HashSet::new();
    let mut assignments = vec![Assignment::default(); people.len()];
    for r in aa {
        let pindex = *ids
            .get(r[0].as_str())
            .ok_or_else(|| format!("人员 {} 不属于本问数据", r[0]))?;
        let p = &people[pindex];
        ensure!(seen.insert(pindex), "人员 {} 重复出现", p.id);
        let blanks = r[1..].iter().filter(|x| x.is_empty()).count();
        if blanks == 4 {
            ensure!(
                q == 3 && p.task == "temporary",
                "人员分配表：人员 {} 为必运人员，四个安排字段不能为空",
                p.id
            );
            continue;
        }
        ensure!(
            blanks == 0,
            "人员分配表：人员 {} 的安排字段填写不完整；{}、flight_no、pickup_stop_order、delivery_stop_order 须同时填写{}",
            p.id,
            rf,
            if q == 3 && p.task == "temporary" {
                "；若未安排该临时人员，请将这四项全部留空"
            } else {
                ""
            }
        );
        ensure!(
            !baseline || p.task != "temporary",
            "基础方案不得安排临时任务"
        );
        let resource = resources
            .iter()
            .position(|x| x == &r[1])
            .ok_or("未知机型或飞机")?;
        let no = number(&r[2], 1)?;
        let f = *index.get(&(resource, no)).ok_or("引用了不存在的架次")?;
        let route = &mut flights[f];
        let a = number(&r[3], 0)?;
        let b = number(&r[4], 0)?;
        ensure!(
            a < b && b < route.stops.len(),
            "须先接后送，停靠序号须在架次范围内"
        );
        ensure!(
            matches(p.origin, route.stops[a].loc) && matches(p.destination, route.stops[b].loc),
            "人员 {} 接送地点不符合需求",
            p.id
        );
        ensure!(
            !route.stops[a + 1..b]
                .iter()
                .any(|s| matches(p.destination, s.loc)),
            "必须在首次到达目的地时下机"
        );
        let start = route.stops[a].dep.unwrap();
        let end = route.stops[b].arr.unwrap();
        ensure!(
            q != 3 || (start >= p.earliest && end <= p.latest),
            "人员 {} 不满足时间窗",
            p.id
        );
        route.board[a] += 1;
        route.alight[b] += 1;
        score[1] += (end - start) as u64;
        if p.task == "temporary" {
            score[7] += 1;
        }
        assignments[pindex] = Assignment {
            served: true,
            flight: f,
            pickup: a,
            delivery: b,
        };
    }
    ensure!(
        seen.len() == people.len(),
        "人员分配表遗漏 {} 名人员；{}",
        people.len() - seen.len(),
        if q == 3 {
            "请提供最终方案的全部人员记录；未安排的临时人员也须保留 person_id，其余四个安排字段留空"
        } else {
            "每名人员都必须有一条安排记录"
        }
    );
    for route in &flights {
        let mut load = 0i64;
        for k in 0..route.stops.len() {
            load -= route.alight[k];
            ensure!(load >= 0, "出现未登机先下机");
            load += route.board[k];
            ensure!(
                load <= d.capacity[route.plane_type] as i64,
                "航段载客超过座位数"
            );
            if k + 1 < route.stops.len() {
                let distance = d.distance[route.stops[k].loc][route.stops[k + 1].loc];
                score[5] += load as u64 * distance;
                score[6] += d.capacity[route.plane_type] * distance;
                if load == 0 {
                    score[8] += distance;
                }
            }
        }
        ensure!(load == 0, "返场后仍有人员未下机");
    }
    ensure!(
        score.iter().all(|x| *x <= u32::MAX as u64),
        "成绩超出证明字段范围"
    );
    Ok(Checked {
        score,
        flights,
        assignments,
        q,
        baseline,
    })
}
