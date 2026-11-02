//! Deterministic AS-Waksman permutation network, including odd-sized networks.
//! Topology adapted from 0xPolygonZero/r1cs, verify_permutation.rs,
//! commit 2e884d71ac290ae95ed58ed8142d71a71b106e2a (MIT OR Apache-2.0).
//! Routing below uses unique row IDs and bipartite coloring; no random challenges.
use super::r1cs::{Builder, Num};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct Row {
    pub id: usize,
    pub fields: [Num; 3],
}

fn settings(a: &[Row], b: &[Row]) -> (Vec<bool>, Vec<bool>) {
    let n = a.len();
    let left = n / 2;
    let right = if n % 2 == 0 { left - 1 } else { left };
    let index: HashMap<_, _> = a.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let positions: Vec<_> = b.iter().map(|r| index[&r.id]).collect();
    let mut graph = vec![Vec::with_capacity(2); n];
    for i in 0..left {
        graph[2 * i].push(2 * i + 1);
        graph[2 * i + 1].push(2 * i);
    }
    for i in 0..right {
        let (x, y) = (positions[2 * i], positions[2 * i + 1]);
        graph[x].push(y);
        graph[y].push(x);
    }
    let mut colors = vec![None; n];
    fn paint(start: usize, value: bool, graph: &[Vec<usize>], colors: &mut [Option<bool>]) {
        let mut pending = vec![(start, value)];
        while let Some((i, v)) = pending.pop() {
            if let Some(old) = colors[i] {
                assert_eq!(old, v, "Waksman routing conflict");
                continue;
            }
            colors[i] = Some(v);
            for j in &graph[i] {
                pending.push((*j, !v));
            }
        }
    }
    if n % 2 == 0 {
        paint(positions[n - 2], false, &graph, &mut colors);
        paint(positions[n - 1], true, &graph, &mut colors);
    } else {
        paint(n - 1, true, &graph, &mut colors);
        paint(positions[n - 1], true, &graph, &mut colors);
    }
    for i in 0..n {
        if colors[i].is_none() {
            paint(i, false, &graph, &mut colors);
        }
    }
    (
        (0..left).map(|i| colors[2 * i].unwrap()).collect(),
        (0..right)
            .map(|i| colors[positions[2 * i]].unwrap())
            .collect(),
    )
}
fn switch(builder: &mut Builder, a: Row, b: Row, swap: bool) -> (Row, Row) {
    let bit = builder.bit(swap);
    let c = std::array::from_fn(|i| builder.select(bit, b.fields[i], a.fields[i]));
    // Explicit second outputs keep all matrix rows sparse at every network depth.
    let d = std::array::from_fn(|i| {
        builder.lin(&[
            (a.fields[i], ark_bn254::Fr::from(1)),
            (b.fields[i], ark_bn254::Fr::from(1)),
            (c[i], -ark_bn254::Fr::from(1)),
        ])
    });
    (
        Row {
            id: if swap { b.id } else { a.id },
            fields: c,
        },
        Row {
            id: if swap { a.id } else { b.id },
            fields: d,
        },
    )
}
pub fn enforce(builder: &mut Builder, a: &[Row], b: &[Row]) {
    assert_eq!(a.len(), b.len());
    let n = a.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        for i in 0..3 {
            builder.eq(a[0].fields[i], b[0].fields[i]);
        }
        return;
    }
    if n == 2 {
        let (c, d) = switch(builder, a[0], a[1], a[0].id != b[0].id);
        for i in 0..3 {
            builder.eq(c.fields[i], b[0].fields[i]);
            builder.eq(d.fields[i], b[1].fields[i]);
        }
        return;
    }
    let (sa, sb) = settings(a, b);
    let (mut a0, mut a1, mut b0, mut b1) = (vec![], vec![], vec![], vec![]);
    for (i, s) in sa.into_iter().enumerate() {
        let (x, y) = switch(builder, a[2 * i], a[2 * i + 1], s);
        a0.push(x);
        a1.push(y);
    }
    for (i, s) in sb.into_iter().enumerate() {
        let (x, y) = switch(builder, b[2 * i], b[2 * i + 1], s);
        b0.push(x);
        b1.push(y);
    }
    if n % 2 == 0 {
        b0.push(b[n - 2]);
        b1.push(b[n - 1]);
    } else {
        a1.push(a[n - 1]);
        b1.push(b[n - 1]);
    }
    enforce(builder, &a0, &b0);
    enforce(builder, &a1, &b1);
}
/// Every key has an official table row. Query values must equal that row, and
/// signed passenger events must balance each row's constrained load difference.
pub fn lookup_and_balance(builder: &mut Builder, rows: &[Row]) {
    let mut sorted = rows.to_vec();
    sorted.sort_by_key(|r| (r.fields[0].integer(), r.id));
    for row in &mut sorted {
        row.fields = row.fields.map(|n| builder.var(n.value));
    }
    enforce(builder, rows, &sorted);
    let mut running = Num::c(0);
    for i in 0..sorted.len() {
        running = builder.add(running, sorted[i].fields[2]);
        if i + 1 < sorted.len() {
            let d = builder.sub(sorted[i + 1].fields[0], sorted[i].fields[0]);
            builder.range(d, 11);
            let same = builder.zero(d);
            builder.gate_eq(same, sorted[i + 1].fields[1], sorted[i].fields[1]);
            let boundary = builder.not(same);
            builder.gate_eq(boundary, running, Num::c(0));
        }
    }
    builder.eq(running, Num::c(0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, seq::SliceRandom};
    #[test]
    fn odd_even_duplicate_tuple_routing() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7654);
        for n in 1..80 {
            for _ in 0..3 {
                let mut cs = Builder::new(&[]);
                let a: Vec<_> = (0..n)
                    .map(|i| Row {
                        id: i,
                        fields: [cs.uint((i % 9) as u64), cs.uint((i % 4) as u64), cs.uint(0)],
                    })
                    .collect();
                let mut b = a.clone();
                b.shuffle(&mut rng);
                for r in &mut b {
                    r.fields = r.fields.map(|v| cs.var(v.value));
                }
                enforce(&mut cs, &a, &b);
                cs.satisfied().unwrap();
                cs.assignment[b[0].fields[1].wire.unwrap()] += ark_bn254::Fr::from(1);
                assert!(cs.satisfied().is_err());
            }
        }
    }
    #[test]
    fn lookup_value_and_event_tampering() {
        for bad in 0..3 {
            let mut cs = Builder::new(&[]);
            let rows = vec![
                Row {
                    id: 0,
                    fields: [Num::c(0), cs.uint(55), Num::c(0)],
                },
                Row {
                    id: 1,
                    fields: [Num::c(1), cs.uint(91), Num::c(0)],
                },
                Row {
                    id: 2,
                    fields: [
                        cs.uint(1),
                        cs.uint(if bad == 1 { 92 } else { 91 }),
                        Num::c(if bad == 2 { 1 } else { 0 }),
                    ],
                },
            ];
            lookup_and_balance(&mut cs, &rows);
            assert_eq!(cs.satisfied().is_ok(), bad == 0);
        }
    }
}
