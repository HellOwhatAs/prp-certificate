//! Fixed PRP final-plan relation, expressed directly as BN254 R1CS.
use super::{
    permutation::{self, Row},
    r1cs::{Builder, Num},
};
use crate::checker::{PROFILE, instance};
use ark_bn254::Fr;
use ark_ff::Field;
use serde::Deserialize;

const S: usize = PROFILE * 7;
#[derive(Clone, Deserialize)]
pub struct Witness {
    pub score: Vec<u64>,
    pub resources: Vec<u64>,
    pub lengths: Vec<u64>,
    pub loc: Vec<u64>,
    pub refuel: Vec<bool>,
    pub arrivals: Vec<u64>,
    pub departures: Vec<u64>,
    pub loads: Vec<u64>,
    pub served: Vec<bool>,
    pub flight: Vec<u64>,
    pub pickup: Vec<u64>,
    pub delivery: Vec<u64>,
}
impl Witness {
    pub fn blank(q: usize) -> Self {
        let p = instance().people[&q.to_string()].len();
        Self {
            score: vec![0; 9],
            resources: vec![0; PROFILE],
            lengths: vec![0; PROFILE],
            loc: vec![0; S],
            refuel: vec![false; S],
            arrivals: vec![0; S],
            departures: vec![0; S],
            loads: vec![0; S],
            served: vec![false; p],
            flight: vec![0; p],
            pickup: vec![0; p],
            delivery: vec![0; p],
        }
    }
}
fn pack(b: &mut Builder, code: Num, length: Num, arr: Num, dep: Num) -> Num {
    b.lin(&[
        (code, Fr::ONE),
        (length, Fr::from(1u64 << 42)),
        (arr, Fr::from(1u64 << 45)),
        (dep, Fr::from(1u64 << 59)),
    ])
}
fn endpoint(b: &mut Builder, location: Num, target: usize) -> Num {
    if target == 55 {
        let xs: Vec<_> = (0..3).map(|i| b.equals(location, Num::c(i))).collect();
        b.sum(&xs)
    } else {
        b.equals(location, Num::c(target as u64))
    }
}
fn clock(b: &mut Builder, time: Num) -> (Num, Num) {
    let day = b.uint(time.integer() / 1440);
    let minute = b.uint(time.integer() % 1440);
    b.range(day, 3);
    b.range(minute, 11);
    b.ge(Num::c(1), Num::c(1439), minute, 11);
    let joined = b.lin(&[(day, Fr::from(1440)), (minute, Fr::ONE)]);
    b.eq(joined, time);
    (day, minute)
}
pub fn compile(q: usize, id: &[u8; 32], w: &Witness) -> Result<Builder, String> {
    if !(1..=3).contains(&q) {
        return Err("题号无效".into());
    }
    let d = instance();
    let people = &d.people[&q.to_string()];
    let p = people.len();
    if w.score.len() != 9
        || w.resources.len() != PROFILE
        || w.lengths.len() != PROFILE
        || [
            w.loc.len(),
            w.refuel.len(),
            w.arrivals.len(),
            w.departures.len(),
            w.loads.len(),
        ]
        .iter()
        .any(|n| *n != S)
        || [
            w.served.len(),
            w.flight.len(),
            w.pickup.len(),
            w.delivery.len(),
        ]
        .iter()
        .any(|n| *n != p)
    {
        return Err("证明见证长度无效".into());
    }
    let mut public: Vec<u64> = id.iter().map(|x| *x as u64).collect();
    public.extend(&w.score);
    let mut b = Builder::new(&public);
    for i in 0..32 {
        b.range(b.public(i), 8);
    }
    for i in 32..41 {
        b.range(b.public(i), 32);
    }
    let locations: Vec<_> = w.loc.iter().map(|n| b.uint(*n)).collect();
    let choices: Vec<_> = locations.iter().map(|n| b.one_hot(*n, 55)).collect();
    let arrival: Vec<_> = w
        .arrivals
        .iter()
        .map(|n| {
            let x = b.uint(*n);
            b.range(x, 14);
            x
        })
        .collect();
    let departure: Vec<_> = w
        .departures
        .iter()
        .map(|n| {
            let x = b.uint(*n);
            b.range(x, 14);
            x
        })
        .collect();
    let load: Vec<_> = w
        .loads
        .iter()
        .map(|n| {
            let x = b.uint(*n);
            b.range(x, 5);
            x
        })
        .collect();
    let refill: Vec<_> = w.refuel.iter().map(|v| b.bit(*v)).collect();
    let mut records = Vec::with_capacity(S + p * 2);
    let mut row_values = vec![Num::c(0); S];
    let mut totals: [Vec<Num>; 9] = std::array::from_fn(|_| vec![]);
    let mut previous_active = Num::c(1);
    let mut previous_resource = Num::c(0);
    let mut previous_end = Num::c(0);
    for f in 0..PROFILE {
        let start = f * 7;
        let length = b.uint(w.lengths[f]);
        let ns = b.one_hot(length, 8);
        b.eq(ns[1], Num::c(0));
        b.eq(ns[2], Num::c(0));
        let active = b.not(ns[0]);
        let previous_inactive = b.not(previous_active);
        b.gate_eq(active, previous_inactive, Num::c(0));
        let resource = b.uint(w.resources[f]);
        let resource_choice = b.one_hot(resource, if q == 3 { 24 } else { 3 });
        b.gate_eq(ns[0], resource, Num::c(0));
        b.ge(active, resource, previous_resource, 5);
        let types: Vec<_> = (0..3)
            .map(|t| {
                if q == 3 {
                    b.sum(
                        &resource_choice
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| d.plane_type[*i] == t)
                            .map(|(_, v)| *v)
                            .collect::<Vec<_>>(),
                    )
                } else {
                    resource_choice[t]
                }
            })
            .collect();
        let capacity = b.dot(&types, &d.capacity);
        let speed = b.dot(&types, &d.speed);
        let rate = b.dot(&types, &d.fuel_rate10);
        let tank = b.dot(
            &types,
            &d.tank10.iter().map(|x| *x as u64).collect::<Vec<_>>(),
        );
        let reserve = b.dot(
            &types,
            &d.reserve10.iter().map(|x| *x as u64).collect::<Vec<_>>(),
        );
        let base_ok = b.sum(&choices[start][..3]);
        b.gate_eq(active, base_ok, Num::c(1));
        if q == 3 {
            let home = b.dot(
                &resource_choice,
                &d.plane_home.iter().map(|x| *x as u64).collect::<Vec<_>>(),
            );
            b.gate_eq(active, locations[start], home);
        }
        let code = b.lin(
            &(0..7)
                .map(|k| (locations[start + k], Fr::from(1u64 << (6 * k))))
                .collect::<Vec<_>>(),
        );
        let mut remaining = tank;
        let mut ends = vec![];
        let mut distances = vec![Num::c(0); 7];
        for k in 1..7 {
            let idx = start + k;
            let mut ds = Vec::with_capacity(55);
            for origin in 0..55 {
                let row = b.dot(&choices[idx], &d.distance[origin]);
                ds.push(b.mul(choices[idx - 1][origin], row));
            }
            distances[k] = b.sum(&ds);
        }
        for k in 0..7 {
            let idx = start + k;
            let present = b.sum(&ns[k + 1..]);
            let absent = b.not(present);
            let last = if k >= 2 { ns[k + 1] } else { Num::c(0) };
            let middle = if k > 0 {
                b.sub(present, last)
            } else {
                Num::c(0)
            };
            let not_middle = b.not(middle);
            for value in [
                locations[idx],
                refill[idx],
                arrival[idx],
                departure[idx],
                load[idx],
            ] {
                b.gate_eq(absent, value, Num::c(0));
            }
            b.gate_eq(not_middle, refill[idx], Num::c(0));
            let allowed = b.sum(
                &d.refuel
                    .iter()
                    .map(|i| choices[idx][*i])
                    .collect::<Vec<_>>(),
            );
            b.gate_eq(refill[idx], allowed, Num::c(1));
            b.ge(Num::c(1), capacity, load[idx], 5);
            b.gate_eq(last, load[idx], Num::c(0));
            b.ge(Num::c(1), Num::c(10080), arrival[idx], 14);
            b.ge(Num::c(1), Num::c(10080), departure[idx], 14);
            if k == 0 {
                b.eq(arrival[idx], Num::c(0));
                if q != 3 {
                    b.eq(departure[idx], Num::c(0));
                }
            } else {
                let same = b.equals(locations[idx], locations[idx - 1]);
                b.gate_eq(present, same, Num::c(0));
                let land = b.sum(&choices[idx][..3]);
                b.gate_eq(middle, land, Num::c(0));
                b.gate_eq(last, locations[idx], locations[start]);
                let fuel = b.mul(distances[k], rate);
                let fuel_and_reserve = b.add(fuel, reserve);
                b.ge(present, remaining, fuel_and_reserve, 20);
                let burned = b.mul(present, fuel);
                totals[2].push(burned);
                let after = b.sub(remaining, burned);
                remaining = b.select(refill[idx], tank, after);
                let speed_n = speed.integer().max(1);
                let fly = b.uint((distances[k].integer() * 60).div_ceil(speed_n));
                b.range(fly, 10);
                let time_speed = b.mul(fly, speed);
                let distance60 = b.scale(distances[k], 60);
                let remainder = b.sub(time_speed, distance60);
                b.range(remainder, 10);
                let speed_less_one = b.sub(speed, Num::c(1));
                b.ge(Num::c(1), speed_less_one, remainder, 10);
                let expected = b.add(departure[idx - 1], fly);
                b.gate_eq(present, arrival[idx], expected);
                b.gate_eq(last, departure[idx], Num::c(0));
                let extra = b.scale(refill[idx], 10);
                let dwell = b.add(Num::c(10), extra);
                let earliest = b.add(arrival[idx], dwell);
                if q == 3 {
                    b.ge(middle, departure[idx], earliest, 14);
                } else {
                    b.gate_eq(middle, departure[idx], earliest);
                }
            }
            ends.push(b.mul(last, arrival[idx]));
            let previous_load = if k == 0 { Num::c(0) } else { load[idx - 1] };
            let table_event = b.sub(previous_load, load[idx]);
            let value = pack(&mut b, code, length, arrival[idx], departure[idx]);
            row_values[idx] = value;
            records.push(Row {
                id: idx,
                fields: [Num::c(idx as u64), value, table_event],
            });
            if k < 6 {
                let leg = b.sum(&ns[k + 2..]);
                let distance = b.mul(leg, distances[k + 1]);
                totals[5].push(b.mul(load[idx], distance));
                totals[6].push(b.mul(capacity, distance));
                let empty = b.zero(load[idx]);
                totals[8].push(b.mul(empty, distance));
            }
        }
        let end = b.sum(&ends);
        let duration = b.sub(end, departure[start]);
        totals[0].push(duration);
        totals[3].push(active);
        let two = b.scale(active, 2);
        totals[4].push(b.sub(length, two));
        if q == 3 {
            let (day, minute) = clock(&mut b, departure[start]);
            b.ge(active, minute, Num::c(360), 11);
            b.ge(active, Num::c(1080), minute, 11);
            let (end_day, end_minute) = clock(&mut b, end);
            b.gate_eq(active, day, end_day);
            b.ge(active, Num::c(1200), end_minute, 11);
            let same = b.equals(resource, previous_resource);
            let continuation = if f == 0 {
                Num::c(0)
            } else {
                b.mul(active, same)
            };
            let ready = b.add(previous_end, Num::c(30));
            b.ge(continuation, departure[start], ready, 14);
        }
        previous_active = active;
        previous_resource = resource;
        previous_end = end;
    }
    let count = b.sum(&totals[3]);
    b.ge(Num::c(1), count, Num::c(1), 9);
    for (person, task) in people.iter().enumerate() {
        let served = b.bit(w.served[person]);
        let absent = b.not(served);
        if q != 3 || task.task != "temporary" {
            b.eq(served, Num::c(1));
        } else {
            totals[7].push(served);
        }
        let flight = b.uint(w.flight[person]);
        b.range(flight, 8);
        let pickup = b.uint(w.pickup[person]);
        let delivery = b.uint(w.delivery[person]);
        let pick = b.one_hot(pickup, 7);
        let drop = b.one_hot(delivery, 7);
        for n in [flight, pickup, delivery] {
            b.gate_eq(absent, n, Num::c(0));
        }
        let f = (w.flight[person] as usize).min(PROFILE - 1);
        let pk = (w.pickup[person] as usize).min(6);
        let dk = (w.delivery[person] as usize).min(6);
        let loc: Vec<_> = (0..7)
            .map(|k| {
                let n = b.uint(w.loc[f * 7 + k]);
                b.range(n, 6);
                n
            })
            .collect();
        let code = b.lin(
            &(0..7)
                .map(|k| (loc[k], Fr::from(1u64 << (6 * k))))
                .collect::<Vec<_>>(),
        );
        let length = b.uint(w.lengths[f]);
        b.range(length, 3);
        let after_pick = b.add(pickup, Num::c(1));
        b.ge(served, delivery, after_pick, 3);
        let after_drop = b.add(delivery, Num::c(1));
        b.ge(served, length, after_drop, 3);
        let mut times = vec![];
        for (endpoint, k, offset) in [(pickup, pk, 0), (delivery, dk, 1)] {
            let idx_base = b.scale(flight, 7);
            let index = b.add(idx_base, endpoint);
            let arr = b.uint(w.arrivals[f * 7 + k]);
            b.range(arr, 14);
            let dep = b.uint(w.departures[f * 7 + k]);
            b.range(dep, 14);
            let value = pack(&mut b, code, length, arr, dep);
            let event = if offset == 0 {
                served
            } else {
                b.sub(Num::c(0), served)
            };
            records.push(Row {
                id: S + person * 2 + offset,
                fields: [index, value, event],
            });
            times.push((arr, dep));
        }
        let pickup_location = b.choose(&pick, &loc);
        let origin_ok = endpoint(&mut b, pickup_location, task.origin);
        b.gate_eq(served, origin_ok, Num::c(1));
        let destinations: Vec<_> = loc
            .iter()
            .map(|n| endpoint(&mut b, *n, task.destination))
            .collect();
        let destination_ok = b.choose(&drop, &destinations);
        b.gate_eq(served, destination_ok, Num::c(1));
        for k in 0..7 {
            let picked = b.sum(&pick[..k]);
            let not_dropped = b.sum(&drop[k + 1..]);
            let inside = b.mul(picked, not_dropped);
            let active_inside = b.mul(served, inside);
            b.gate_eq(active_inside, destinations[k], Num::c(0));
        }
        b.ge(served, times[1].0, times[0].1, 14);
        let elapsed = b.sub(times[1].0, times[0].1);
        totals[1].push(b.mul(served, elapsed));
        if q == 3 {
            b.ge(served, times[0].1, Num::c(task.earliest as u64), 14);
            b.ge(served, Num::c(task.latest as u64), times[1].0, 14);
        }
    }
    permutation::lookup_and_balance(&mut b, &records);
    for (i, terms) in totals.iter().enumerate() {
        let total = b.sum(terms);
        b.eq(total, b.public(32 + i));
    }
    Ok(b)
}
