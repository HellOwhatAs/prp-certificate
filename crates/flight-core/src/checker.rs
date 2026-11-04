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
    line: usize,
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
    Ok(parse_csv_rows(text, header, max_rows)?
        .into_iter()
        .map(|(_, row)| row)
        .collect())
}

// Retain each record's physical starting line, including across blank lines and
// quoted multiline fields. The public parser still returns only the field data.
fn parse_csv_rows(
    text: &str,
    header: &[&str],
    max_rows: usize,
) -> Result<Vec<(usize, Vec<String>)>, String> {
    ensure!(
        text.len() <= 10_000_000,
        "CSV 大小为 {} 字节，超过 10 MB（10000000 字节）上限",
        text.len()
    );
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut line = 1;
    let mut row_line = 1;
    let mut quote_line = 1;
    let mut previous_cr = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        let at_line = line;
        if c == '\r' || (c == '\n' && !previous_cr) {
            line += 1;
        }
        previous_cr = c == '\r';
        ensure!(
            c != '\0',
            "CSV 第 {at_line} 行第 {} 列含 NUL 字符，须删除该字符",
            row.len() + 1
        );
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
            ensure!(
                field.is_empty() && !closed,
                "CSV 第 {at_line} 行第 {} 列引号格式错误：双引号只能出现在字段开头；字段内的双引号须写成两个双引号",
                row.len() + 1
            );
            quoted = true;
            quote_line = at_line;
        } else if c == ',' || c == '\n' || c == '\r' {
            row.push(std::mem::take(&mut field));
            closed = false;
            if c != ',' {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                    previous_cr = false;
                }
                if !(row.len() == 1 && row[0].is_empty()) {
                    rows.push((row_line, std::mem::take(&mut row)));
                } else {
                    row.clear();
                }
                ensure!(
                    rows.len() <= max_rows + 1,
                    "CSV 第 {row_line} 行使数据记录数超过上限 {max_rows}（不含表头）"
                );
                row_line = line;
            }
        } else {
            ensure!(
                !closed,
                "CSV 第 {at_line} 行第 {} 列引号后的内容 {:?} 无效：闭合引号后只能是逗号、换行或文件结束",
                row.len() + 1,
                c
            );
            field.push(c);
        }
    }
    ensure!(
        !quoted,
        "CSV 第 {quote_line} 行第 {} 列引号未闭合（记录从第 {row_line} 行开始）",
        row.len() + 1
    );
    if !field.is_empty() || !row.is_empty() || closed {
        row.push(field);
        rows.push((row_line, row));
    }
    ensure!(!rows.is_empty(), "CSV 缺少表头；应为：{}", header.join(","));
    ensure!(
        rows[0]
            .1
            .iter()
            .map(String::as_str)
            .eq(header.iter().copied()),
        "CSV 第 {} 行表头不一致；实际：{:?}；应为：{}",
        rows[0].0,
        rows[0].1.join(","),
        header.join(",")
    );
    rows.remove(0);
    ensure!(
        rows.len() <= max_rows,
        "CSV 第 {row_line} 行使数据记录数达到 {}，超过上限 {max_rows}（不含表头）",
        rows.len()
    );
    for (line, row) in &rows {
        ensure!(
            row.len() == header.len(),
            "CSV 第 {line} 行数据列数与表头不一致：实际 {} 列，应为 {} 列（{}）",
            row.len(),
            header.len(),
            header.join(",")
        );
    }
    Ok(rows)
}
fn number(s: &str, min: usize) -> Result<usize, String> {
    ensure!(
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')),
        "整数格式无效：{s:?}；须为不含前导零的十进制整数"
    );
    let n = s
        .parse::<usize>()
        .map_err(|_| format!("整数 {s:?} 超出范围；允许范围为 {min}–{}", u32::MAX))?;
    ensure!(
        n >= min && n <= u32::MAX as usize,
        "整数 {s:?} 超出范围；允许范围为 {min}–{}",
        u32::MAX
    );
    Ok(n)
}
fn planning_start() -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 8, 3)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
}
fn time_label(minutes: i64) -> String {
    (planning_start() + chrono::Duration::minutes(minutes))
        .format("%Y-%m-%d %H:%M")
        .to_string()
}
fn optional_time_label(minutes: Option<i64>) -> String {
    minutes.map(time_label).unwrap_or_else(|| "留空".into())
}
fn minute(s: &str) -> Result<Option<i64>, String> {
    if s.is_empty() {
        return Ok(None);
    }
    let d = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M")
        .map_err(|_| format!("时间 {s:?} 无效；格式必须为 YYYY-MM-DD HH:MM"))?;
    ensure!(
        d.format("%Y-%m-%d %H:%M").to_string() == s,
        "日期时间 {s:?} 格式无效；须严格使用 YYYY-MM-DD HH:MM"
    );
    let n = (d - planning_start()).num_minutes();
    ensure!(
        (0..=10080).contains(&n),
        "时刻 {s:?} 超出规划期；允许范围为 {} 至 {}（规划期第 0–10080 分钟）",
        time_label(0),
        time_label(10080)
    );
    Ok(Some(n))
}
fn stop_context(d: &Instance, resource: &str, no: usize, stop: &Stop) -> String {
    format!(
        "航线表第 {} 行（{}，flight_no={}，stop_order={}，facility_id={}）",
        stop.line, resource, no, stop.k, d.locations[stop.loc]
    )
}
fn endpoint_label(d: &Instance, endpoint: usize) -> &str {
    if endpoint == 55 {
        "任一机场（LAND）"
    } else {
        &d.locations[endpoint]
    }
}
fn matches(endpoint: usize, location: usize) -> bool {
    if endpoint == 55 {
        location < 3
    } else {
        endpoint == location
    }
}
pub fn check(q: usize, routes: &str, assignments: &str, baseline: bool) -> Result<Checked, String> {
    ensure!((1..=3).contains(&q), "不支持的题号 {q}；只支持 1、2、3");
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
    let rr = parse_csv_rows(routes, &rh, 40000).map_err(|e| format!("航线表：{e}"))?;
    let aa = parse_csv_rows(
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
    ensure!(!rr.is_empty(), "航线表为空；至少须有一条航线记录");
    ensure!(!aa.is_empty(), "人员分配表为空；须提供本问全部人员记录");
    let types = vec!["T1".to_owned(), "T2".into(), "T3".into()];
    let resources = if q == 3 { &d.planes } else { &types };
    let mut map: BTreeMap<(usize, usize), Flight> = BTreeMap::new();
    for (line, r) in &rr {
        let resource = resources
            .iter()
            .position(|x| x == &r[0])
            .ok_or_else(|| format!("航线表第 {line} 行：{rf}={:?}，未知机型或飞机", r[0]))?;
        let no = number(&r[1], 1).map_err(|e| format!("航线表第 {line} 行：flight_no {e}"))?;
        let k = number(&r[2], 0).map_err(|e| format!("航线表第 {line} 行：stop_order {e}"))?;
        let context = format!(
            "航线表第 {line} 行（{}，flight_no={no}，stop_order={k}）",
            r[0]
        );
        let loc = d
            .locations
            .iter()
            .position(|x| x == &r[3])
            .ok_or_else(|| format!("{context}：facility_id={:?}，未知地点", r[3]))?;
        let fuel = r.last().unwrap();
        ensure!(
            fuel == "0" || fuel == "1",
            "{context}：refuel={fuel:?}，必须为 0 或 1"
        );
        let refuel = fuel == "1";
        ensure!(
            !refuel || d.refuel.contains(&loc),
            "{context}：facility_id={} 不允许加油，refuel 应为 0",
            d.locations[loc]
        );
        let route = map.entry((resource, no)).or_insert_with(|| Flight {
            resource,
            no,
            ..Default::default()
        });
        if let Some(previous) = route.stops.iter().find(|s| s.k == k) {
            return Err(format!(
                "{context}：stop_order={k} 重复，首次出现在航线表第 {} 行",
                previous.line
            ));
        }
        route.stops.push(Stop {
            line: *line,
            k,
            loc,
            refuel,
            arr: if q == 3 {
                minute(&r[4]).map_err(|e| format!("{context}：arrival_time {e}"))?
            } else {
                None
            },
            dep: if q == 3 {
                minute(&r[5]).map_err(|e| format!("{context}：departure_time {e}"))?
            } else {
                None
            },
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
        let flight = format!("航线表（{}，flight_no={}）", resources[r], route.no);
        counts[r] += 1;
        ensure!(
            route.no == counts[r],
            "{flight}：flight_no 必须从 1 连续编号；实际 {}，应为 {}",
            route.no,
            counts[r]
        );
        route.stops.sort_by_key(|s| s.k);
        let n = route.stops.len();
        ensure!(
            (3..=7).contains(&n),
            "{flight}：实际有 {n} 条停靠记录，应有 3–7 条（含出发、返回机场及 1–5 次海上停靠）"
        );
        for (expected, stop) in route.stops.iter().enumerate() {
            ensure!(
                expected == stop.k,
                "{}：stop_order 必须从 0 连续编号；实际 {}，应为 {expected}",
                stop_context(d, &resources[r], route.no, stop),
                stop.k
            );
        }
        let base = route.stops[0].loc;
        ensure!(
            base < 3 && route.stops[n - 1].loc == base,
            "{flight}：必须从同一机场出发并返回；实际出发地点 {}，返回地点 {}",
            d.locations[base],
            d.locations[route.stops[n - 1].loc]
        );
        ensure!(
            q != 3 || base == d.plane_home[r],
            "{flight}：必须从所属机场出发；实际 {}，所属机场为 {}",
            d.locations[base],
            d.locations[d.plane_home[r]]
        );
        for stop in &route.stops[1..n - 1] {
            ensure!(
                stop.loc >= 3,
                "{}：中途停靠必须为海上设施，实际为机场 {}",
                stop_context(d, &resources[r], route.no, stop),
                d.locations[stop.loc]
            );
        }
        ensure!(
            !route.stops[0].refuel && !route.stops[n - 1].refuel,
            "{flight}：机场停靠不能标记加油；首站 refuel={}，末站 refuel={}，应均为 0",
            u8::from(route.stops[0].refuel),
            u8::from(route.stops[n - 1].refuel)
        );
        let t = if q == 3 { d.plane_type[r] } else { r };
        route.plane_type = t;
        let mut remaining = d.tank10[t];
        route.board = vec![0; n];
        route.alight = vec![0; n];
        for k in 0..n {
            let s = route.stops[k].clone();
            let context = stop_context(d, &resources[r], route.no, &s);
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
                    "{context}：到达/离开时刻填写不正确；arrival_time={}，departure_time={}；{}",
                    optional_time_label(s.arr),
                    optional_time_label(s.dep),
                    if k == 0 {
                        "首站 arrival_time 须留空、departure_time 须填写"
                    } else if last {
                        "末站 arrival_time 须填写、departure_time 须留空"
                    } else {
                        "中途停靠的 arrival_time、departure_time 均须填写"
                    }
                );
            } else if k == 0 {
                route.stops[k].arr = Some(0);
                route.stops[k].dep = Some(0);
            }
            if k > 0 {
                let prev = &route.stops[k - 1];
                let leg = format!(
                    "航段 stop_order={}（{}）→{}（{}）",
                    k - 1,
                    d.locations[prev.loc],
                    k,
                    d.locations[s.loc]
                );
                ensure!(prev.loc != s.loc, "{context}：{leg} 不能连续停靠同一地点");
                let distance = d.distance[prev.loc][s.loc];
                let fuel = distance * d.fuel_rate10[t];
                remaining -= fuel as i64;
                score[2] += fuel;
                ensure!(
                    remaining >= d.reserve10[t],
                    "{context}：{leg} 到达余油 {:.1} kg，低于安全余油 {:.1} kg；本段距离 {distance} km，耗油 {:.1} kg",
                    remaining as f64 / 10.,
                    d.reserve10[t] as f64 / 10.,
                    fuel as f64 / 10.
                );
                if s.refuel {
                    remaining = d.tank10[t];
                }
                let expected = prev
                    .dep
                    .ok_or_else(|| format!("{context}：{leg} 的上一停靠点缺少 departure_time"))?
                    + (60 * distance).div_ceil(d.speed[t]) as i64;
                if q == 3 {
                    ensure!(
                        s.arr == Some(expected),
                        "{context}：{leg} 到达时刻与航程不一致；实际 {}，应为 {}（规划期第 {expected} 分钟，距离 {distance} km，速度 {} km/h）",
                        optional_time_label(s.arr),
                        time_label(expected),
                        d.speed[t]
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
                    let dwell = route.stops[k].dep.unwrap() - route.stops[k].arr.unwrap();
                    let minimum = if s.refuel { 20 } else { 10 };
                    ensure!(
                        dwell >= minimum,
                        "{context}：中途停留时间不足；实际 {dwell} 分钟，至少需要 {minimum} 分钟（refuel={}，到达 {}，离开 {}）",
                        u8::from(s.refuel),
                        optional_time_label(route.stops[k].arr),
                        optional_time_label(route.stops[k].dep)
                    );
                }
            }
        }
        let start = route.stops[0].dep.unwrap();
        let end = route.stops[n - 1].arr.unwrap();
        if q == 3 {
            ensure!(
                (360..=1080).contains(&(start % 1440)),
                "{flight}：起飞时刻 {} 不在允许时段内；须在每天 06:00–18:00 起飞",
                time_label(start)
            );
            ensure!(
                start / 1440 == end / 1440 && end % 1440 <= 1200,
                "{flight}：须在起飞当天 20:00 前返回；实际起飞 {}，返回 {}，最晚返回 {}",
                time_label(start),
                time_label(end),
                time_label(start / 1440 * 1440 + 1200)
            );
            if let Some(prev) = last_end[r] {
                ensure!(
                    start >= prev + 30,
                    "{flight}：架次冲突或机场周转不足；上一架次 flight_no={} 返回 {}，本次起飞 {}，实际间隔 {} 分钟，至少需要 30 分钟（最早起飞 {}）",
                    route.no - 1,
                    time_label(prev),
                    time_label(start),
                    start - prev,
                    time_label(prev + 30)
                );
            }
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
    for (line, r) in aa {
        let pindex = *ids.get(r[0].as_str()).ok_or_else(|| {
            format!(
                "人员分配表第 {line} 行：person_id={:?} 不属于第 {q} 问数据",
                r[0]
            )
        })?;
        let p = &people[pindex];
        let context = format!("人员分配表第 {line} 行（person_id={}）", p.id);
        ensure!(
            seen.insert(pindex),
            "{context}：人员重复出现，每人只能有一条记录"
        );
        let blanks = r[1..].iter().filter(|x| x.is_empty()).count();
        if blanks == 4 {
            ensure!(
                q == 3 && p.task == "temporary",
                "{context}：该人员为必运人员，四个安排字段不能为空"
            );
            continue;
        }
        ensure!(
            blanks == 0,
            "{context}：安排字段填写不完整（实际有 {blanks} 项留空）；{rf}、flight_no、pickup_stop_order、delivery_stop_order 须同时填写{}",
            if q == 3 && p.task == "temporary" {
                "；若未安排该临时人员，请将这四项全部留空"
            } else {
                ""
            }
        );
        ensure!(
            !baseline || p.task != "temporary",
            "{context}：基础方案不得安排临时任务"
        );
        let resource = resources
            .iter()
            .position(|x| x == &r[1])
            .ok_or_else(|| format!("{context}：{rf}={:?}，未知机型或飞机", r[1]))?;
        let no = number(&r[2], 1).map_err(|e| format!("{context}：flight_no {e}"))?;
        let context = format!("{context}，{}，flight_no={no}", resources[resource]);
        let f = *index
            .get(&(resource, no))
            .ok_or_else(|| format!("{context}：引用了航线表中不存在的架次"))?;
        let route = &mut flights[f];
        let a = number(&r[3], 0).map_err(|e| format!("{context}：pickup_stop_order {e}"))?;
        let b = number(&r[4], 0).map_err(|e| format!("{context}：delivery_stop_order {e}"))?;
        ensure!(
            a < b && b < route.stops.len(),
            "{context}：须先接后送，停靠序号须在架次范围内；实际 pickup_stop_order={a}、delivery_stop_order={b}，应满足 0 ≤ 接人序号 < 送达序号 ≤ {}",
            route.stops.len() - 1
        );
        ensure!(
            matches(p.origin, route.stops[a].loc) && matches(p.destination, route.stops[b].loc),
            "{context}：接送地点不符合需求；实际 stop_order={a}（{}）→{b}（{}），需求为 {}→{}",
            d.locations[route.stops[a].loc],
            d.locations[route.stops[b].loc],
            endpoint_label(d, p.origin),
            endpoint_label(d, p.destination)
        );
        if let Some(first) = route.stops[a + 1..b]
            .iter()
            .find(|s| matches(p.destination, s.loc))
        {
            return Err(format!(
                "{context}：必须在首次到达目的地时下机；实际 delivery_stop_order={b}，首次到达目的地 {} 在 stop_order={}",
                d.locations[first.loc], first.k
            ));
        }
        let start = route.stops[a].dep.unwrap();
        let end = route.stops[b].arr.unwrap();
        ensure!(
            q != 3 || (start >= p.earliest && end <= p.latest),
            "{context}：不满足时间窗；实际在 stop_order={a} 于 {} 接人、stop_order={b} 于 {} 送达；最早可接人 {}，最晚须送达 {}",
            time_label(start),
            time_label(end),
            time_label(p.earliest),
            time_label(p.latest)
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
        "人员分配表遗漏 {} 名人员（例如 {}）；{}",
        people.len() - seen.len(),
        people
            .iter()
            .enumerate()
            .filter(|(i, _)| !seen.contains(i))
            .take(5)
            .map(|(_, p)| p.id.as_str())
            .collect::<Vec<_>>()
            .join("、"),
        if q == 3 {
            "请提供最终方案的全部人员记录；未安排的临时人员也须保留 person_id，其余四个安排字段留空"
        } else {
            "每名人员都必须有一条安排记录"
        }
    );
    for route in &flights {
        let mut load = 0i64;
        for k in 0..route.stops.len() {
            let context = stop_context(d, &resources[route.resource], route.no, &route.stops[k]);
            load -= route.alight[k];
            ensure!(
                load >= 0,
                "{context}：出现未登机先下机，下机后人数为 {load}，应不少于 0"
            );
            load += route.board[k];
            ensure!(
                load <= d.capacity[route.plane_type] as i64,
                "{context}：{}载客 {load} 人，超过机型 T{} 的 {} 个座位",
                if k + 1 < route.stops.len() {
                    format!(
                        "飞往 stop_order={}（{}）的航段",
                        k + 1,
                        d.locations[route.stops[k + 1].loc]
                    )
                } else {
                    "停靠后".into()
                },
                route.plane_type + 1,
                d.capacity[route.plane_type]
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
        ensure!(
            load == 0,
            "航线表（{}，flight_no={}）：返场后仍有 {load} 名人员未下机，应为 0",
            resources[route.resource],
            route.no
        );
    }
    ensure!(
        score.iter().all(|x| *x <= u32::MAX as u64),
        "成绩超出证明字段范围；各项成绩须不超过 {}，实际成绩为 {:?}",
        u32::MAX,
        score
    );
    Ok(Checked {
        score,
        flights,
        assignments,
        q,
        baseline,
    })
}
