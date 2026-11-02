//! Small deterministic R1CS builder. No witness-dependent constraint generation.
use ark_bn254::Fr;
use ark_ff::{Field, PrimeField, Zero};
use ark_relations::gr1cs::{
    ConstraintSynthesizer, ConstraintSystemRef, LinearCombination, SynthesisError, Variable,
};
use sha2::{Digest, Sha256};

pub type Row = Vec<(Fr, usize)>;
#[derive(Clone, Copy, Debug)]
pub struct Num {
    pub wire: Option<usize>,
    pub value: Fr,
}
impl Num {
    pub fn c(n: u64) -> Self {
        Self {
            wire: None,
            value: Fr::from(n),
        }
    }
    pub fn integer(self) -> u64 {
        self.value.into_bigint().0[0]
    }
}
pub struct Builder {
    pub assignment: Vec<Fr>,
    pub matrices: [Vec<Row>; 3],
    pub inputs: usize,
    boolean: Vec<bool>,
}
impl Builder {
    pub fn into_parts(self) -> (Vec<Fr>, [Vec<Row>; 3], usize) {
        (self.assignment, self.matrices, self.inputs)
    }
    pub fn new(public: &[u64]) -> Self {
        let mut assignment = vec![Fr::ONE];
        assignment.extend(public.iter().copied().map(Fr::from));
        let boolean = vec![false; assignment.len()];
        Self {
            assignment,
            matrices: std::array::from_fn(|_| Vec::new()),
            inputs: public.len() + 1,
            boolean,
        }
    }
    pub fn public(&self, i: usize) -> Num {
        Num {
            wire: Some(i + 1),
            value: self.assignment[i + 1],
        }
    }
    pub fn var(&mut self, value: Fr) -> Num {
        let wire = self.assignment.len();
        self.assignment.push(value);
        self.boolean.push(false);
        Num {
            wire: Some(wire),
            value,
        }
    }
    pub fn uint(&mut self, value: u64) -> Num {
        self.var(Fr::from(value))
    }
    fn row(terms: &[(Num, Fr)]) -> Row {
        let mut out: Row = terms
            .iter()
            .map(|(n, c)| match n.wire {
                Some(i) => (*c, i),
                None => (*c * n.value, 0),
            })
            .filter(|(c, _)| !c.is_zero())
            .collect();
        out.sort_unstable_by_key(|t| t.1);
        let mut combined: Row = Vec::with_capacity(out.len());
        for (c, i) in out {
            if combined.last().is_some_and(|x| x.1 == i) {
                combined.last_mut().unwrap().0 += c;
            } else {
                combined.push((c, i));
            }
        }
        combined.retain(|x| !x.0.is_zero());
        combined
    }
    pub fn enforce(&mut self, a: &[(Num, Fr)], b: &[(Num, Fr)], c: &[(Num, Fr)]) {
        let mut a = Self::row(a);
        let mut b = Self::row(b);
        // A*B is symmetric. Prefer constants/reused Boolean wires in B so the
        // expensive G2 proving-key query has far fewer nonzero entries.
        let rank = |r: &Row| {
            if r.iter().all(|(_, i)| *i == 0) {
                0
            } else if r.iter().all(|(_, i)| *i == 0 || self.boolean[*i]) {
                1
            } else {
                2
            }
        };
        if rank(&a) < rank(&b) {
            std::mem::swap(&mut a, &mut b);
        }
        self.matrices[0].push(a);
        self.matrices[1].push(b);
        self.matrices[2].push(Self::row(c));
    }
    pub fn lin(&mut self, terms: &[(Num, Fr)]) -> Num {
        let value = terms.iter().map(|(n, c)| n.value * c).sum();
        if terms.iter().all(|(n, _)| n.wire.is_none()) {
            return Num { wire: None, value };
        }
        let out = self.var(value);
        let mut a = terms.to_vec();
        a.push((out, -Fr::ONE));
        self.enforce(&a, &[(Num::c(1), Fr::ONE)], &[]);
        out
    }
    pub fn sum(&mut self, ns: &[Num]) -> Num {
        self.lin(&ns.iter().map(|n| (*n, Fr::ONE)).collect::<Vec<_>>())
    }
    pub fn add(&mut self, a: Num, b: Num) -> Num {
        self.lin(&[(a, Fr::ONE), (b, Fr::ONE)])
    }
    pub fn sub(&mut self, a: Num, b: Num) -> Num {
        self.lin(&[(a, Fr::ONE), (b, -Fr::ONE)])
    }
    pub fn scale(&mut self, a: Num, k: u64) -> Num {
        self.lin(&[(a, Fr::from(k))])
    }
    pub fn mul(&mut self, a: Num, b: Num) -> Num {
        if a.wire.is_none() {
            return self.lin(&[(b, a.value)]);
        }
        if b.wire.is_none() {
            return self.lin(&[(a, b.value)]);
        }
        let out = self.var(a.value * b.value);
        self.enforce(&[(a, Fr::ONE)], &[(b, Fr::ONE)], &[(out, Fr::ONE)]);
        out
    }
    pub fn eq(&mut self, a: Num, b: Num) {
        self.enforce(&[(a, Fr::ONE), (b, -Fr::ONE)], &[(Num::c(1), Fr::ONE)], &[]);
    }
    pub fn gate_eq(&mut self, gate: Num, a: Num, b: Num) {
        self.enforce(&[(gate, Fr::ONE)], &[(a, Fr::ONE), (b, -Fr::ONE)], &[]);
    }
    pub fn bit(&mut self, value: bool) -> Num {
        let b = self.uint(u64::from(value));
        self.boolean[b.wire.unwrap()] = true;
        self.enforce(&[(b, Fr::ONE)], &[(b, Fr::ONE), (Num::c(1), -Fr::ONE)], &[]);
        b
    }
    pub fn not(&mut self, a: Num) -> Num {
        self.sub(Num::c(1), a)
    }
    pub fn select(&mut self, gate: Num, a: Num, b: Num) -> Num {
        let out = self.var(b.value + gate.value * (a.value - b.value));
        self.enforce(
            &[(gate, Fr::ONE)],
            &[(a, Fr::ONE), (b, -Fr::ONE)],
            &[(out, Fr::ONE), (b, -Fr::ONE)],
        );
        out
    }
    pub fn zero(&mut self, a: Num) -> Num {
        let inv = self.var(a.value.inverse().unwrap_or(Fr::zero()));
        let z = self.uint(u64::from(a.value.is_zero()));
        self.enforce(
            &[(a, Fr::ONE)],
            &[(inv, Fr::ONE)],
            &[(Num::c(1), Fr::ONE), (z, -Fr::ONE)],
        );
        self.enforce(&[(a, Fr::ONE)], &[(z, Fr::ONE)], &[]);
        z
    }
    pub fn equals(&mut self, a: Num, b: Num) -> Num {
        let d = self.sub(a, b);
        self.zero(d)
    }
    pub fn range(&mut self, n: Num, bits: usize) -> Vec<Num> {
        assert!(bits < 64);
        let value = n.integer();
        let mut terms = Vec::with_capacity(bits + 1);
        let mut out = vec![];
        for i in 0..bits {
            let b = self.bit((value >> i) & 1 == 1);
            terms.push((b, Fr::from(1u64 << i)));
            out.push(b);
        }
        terms.push((n, -Fr::ONE));
        self.enforce(&terms, &[(Num::c(1), Fr::ONE)], &[]);
        out
    }
    pub fn ge(&mut self, gate: Num, a: Num, b: Num, bits: usize) {
        let d = self.var(gate.value * (a.value - b.value));
        self.enforce(
            &[(gate, Fr::ONE)],
            &[(a, Fr::ONE), (b, -Fr::ONE)],
            &[(d, Fr::ONE)],
        );
        self.range(d, bits);
    }
    pub fn one_hot(&mut self, n: Num, count: usize) -> Vec<Num> {
        let out: Vec<_> = (0..count)
            .map(|i| self.bit(n.value == Fr::from(i as u64)))
            .collect();
        let sum = self.sum(&out);
        self.eq(sum, Num::c(1));
        let index = self.lin(
            &out.iter()
                .enumerate()
                .map(|(i, b)| (*b, Fr::from(i as u64)))
                .collect::<Vec<_>>(),
        );
        self.eq(index, n);
        out
    }
    pub fn dot(&mut self, selector: &[Num], data: &[u64]) -> Num {
        self.lin(
            &selector
                .iter()
                .zip(data)
                .map(|(b, n)| (*b, Fr::from(*n)))
                .collect::<Vec<_>>(),
        )
    }
    pub fn choose(&mut self, selector: &[Num], data: &[Num]) -> Num {
        let terms: Vec<_> = selector
            .iter()
            .zip(data)
            .map(|(b, n)| self.mul(*b, *n))
            .collect();
        self.sum(&terms)
    }
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({"constraints":self.matrices[0].len(),"variables":self.assignment.len(),"public_inputs":self.inputs-1,"nonzeros":self.matrices.iter().map(|m|m.iter().map(Vec::len).sum::<usize>()).collect::<Vec<_>>()})
    }
    pub fn digest(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"PRP-GROTH16-R1CS-1");
        h.update((self.inputs as u64).to_le_bytes());
        h.update((self.assignment.len() as u64).to_le_bytes());
        for matrix in &self.matrices {
            h.update((matrix.len() as u64).to_le_bytes());
            for row in matrix {
                h.update((row.len() as u64).to_le_bytes());
                for (c, i) in row {
                    h.update((*i as u64).to_le_bytes());
                    for limb in c.into_bigint().0 {
                        h.update(limb.to_le_bytes());
                    }
                }
            }
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
    pub fn satisfied(&self) -> Result<(), String> {
        let eval = |row: &Row| {
            row.iter()
                .map(|(c, i)| *c * self.assignment[*i])
                .sum::<Fr>()
        };
        for i in 0..self.matrices[0].len() {
            if eval(&self.matrices[0][i]) * eval(&self.matrices[1][i]) != eval(&self.matrices[2][i])
            {
                return Err(format!("R1CS constraint {i} failed"));
            }
        }
        Ok(())
    }
}
impl ConstraintSynthesizer<Fr> for Builder {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        for i in 1..self.inputs {
            let _ = cs.new_input_variable(|| Ok(self.assignment[i]))?;
        }
        for i in self.inputs..self.assignment.len() {
            let _ = cs.new_witness_variable(|| Ok(self.assignment[i]))?;
        }
        let map = |row: Row| -> LinearCombination<Fr> {
            LinearCombination(
                row.into_iter()
                    .map(|(c, i)| {
                        (
                            c,
                            if i == 0 {
                                Variable::one()
                            } else if i < self.inputs {
                                Variable::instance(i)
                            } else {
                                Variable::witness(i - self.inputs)
                            },
                        )
                    })
                    .collect(),
            )
        };
        let [aa, bb, cc] = self.matrices;
        for ((a, b), c) in aa.into_iter().zip(bb).zip(cc) {
            cs.enforce_r1cs_constraint(|| map(a), || map(b), || map(c))?;
        }
        Ok(())
    }
}
