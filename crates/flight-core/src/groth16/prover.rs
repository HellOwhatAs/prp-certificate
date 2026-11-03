//! Memory-bounded scheduling of the arkworks 0.6 LibsnarkReduction and Groth16
//! equations (ark-groth16/src/{r1cs_to_qap,prover}.rs, MIT OR Apache-2.0).
//! Same QAP, setup and fresh r/s; matrices are consumed before FFT/MSM and MSM
//! inputs are chunked. No weakened constraints or non-ZK proving mode.
use super::r1cs::Builder;
use ark_bn254::{Bn254, Fr, G1Projective, G2Projective};
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{FftField, Field, PrimeField, Zero};
use ark_groth16::{Proof, ProvingKey, r1cs_to_qap::evaluate_constraint};
use ark_poly::{EvaluationDomain, GeneralEvaluationDomain};
#[cfg(any(not(target_arch = "wasm32"), feature = "web-threads"))]
use rayon::prelude::*;

fn msm<G: CurveGroup<ScalarField = Fr>>(bases: &[G::Affine], scalars: &[Fr]) -> G {
    assert_eq!(bases.len(), scalars.len());
    // Ark's full-size signed-window MSM materializes n*window-count digits.
    // Bound its temporary storage independently of the number of constraints.
    let chunk = |(points, values): (&[G::Affine], &[Fr])| {
        let mut p = Vec::with_capacity(points.len());
        let mut s = Vec::with_capacity(points.len());
        for (point, value) in points.iter().zip(values) {
            if !point.is_zero() && !value.is_zero() {
                p.push(*point);
                s.push(value.into_bigint());
            }
        }
        G::msm_bigint(&p, &s)
    };
    // Bound each task's temporary storage and keep progress/JS on the caller.
    // Parallelize whole MSM chunks, avoiding per-scalar cross-thread allocations.
    #[cfg(all(target_arch = "wasm32", feature = "web-threads"))]
    return bases
        .par_chunks(65536)
        .zip(scalars.par_chunks(65536))
        .map(chunk)
        .reduce(G::zero, |a, b| a + b);
    #[cfg(not(all(target_arch = "wasm32", feature = "web-threads")))]
    bases
        .chunks(65536)
        .zip(scalars.chunks(65536))
        .map(chunk)
        .fold(G::zero(), |a, b| a + b)
}

pub fn prove(
    pk: ProvingKey<Bn254>,
    b: Builder,
    r: Fr,
    s: Fr,
    progress: impl FnMut(&str),
) -> Result<Proof<Bn254>, String> {
    prove_prepared(pk, prepare(b)?, r, s, progress)
}

// Private fields ensure callers cannot skip constraint validation or mutate the
// checked evaluations before proving. No witness/matrix cache survives a job.
pub(crate) struct Prepared {
    assignment: Vec<Fr>,
    polynomials: [Vec<Fr>; 3],
    inputs: usize,
    domain: GeneralEvaluationDomain<Fr>,
}

pub(crate) fn prepare(b: Builder) -> Result<Prepared, String> {
    let (assignment, matrices, inputs) = b.into_parts();
    let count = matrices[0].len();
    if matrices.iter().any(|rows| rows.len() != count) || inputs > assignment.len() {
        return Err("证明约束维度不匹配".into());
    }
    let domain = GeneralEvaluationDomain::<Fr>::new(count + inputs).ok_or("证明多项式过大")?;
    let size = domain.size();
    // Evaluate each row ONCE for both satisfaction checking and the QAP.
    // Schedule independent rows and use arkworks' evaluator. Drop each matrix
    // on the caller to avoid contention in the WASM allocator; all sparse
    // matrices are freed before FFT/MSM, as in the original bounded prover.
    let mut polynomials = matrices.map(|rows| {
        let mut values = vec![Fr::zero(); size];
        for_each_pair(&mut values[..count], &rows, |value, row| {
            *value = evaluate_constraint(row, &assignment);
        });
        values
    });
    let [a, b, c] = &polynomials;
    if let Some(i) = first_unsatisfied(&a[..count], &b[..count], &c[..count]) {
        return Err(format!("R1CS constraint {i} failed"));
    }
    polynomials[0][count..count + inputs].copy_from_slice(&assignment[..inputs]);
    Ok(Prepared {
        assignment,
        polynomials,
        inputs,
        domain,
    })
}

pub(crate) fn prove_prepared(
    pk: ProvingKey<Bn254>,
    prepared: Prepared,
    r: Fr,
    s: Fr,
    mut progress: impl FnMut(&str),
) -> Result<Proof<Bn254>, String> {
    let Prepared {
        assignment,
        polynomials: [mut a, mut b, mut c],
        inputs,
        domain,
    } = prepared;
    progress("计算证明多项式…");
    domain.ifft_in_place(&mut a);
    domain.ifft_in_place(&mut b);
    let coset = domain.get_coset(Fr::GENERATOR).ok_or("证明计算域无效")?;
    coset.fft_in_place(&mut a);
    coset.fft_in_place(&mut b);
    for_each_pair(&mut a, &b, |a, b| *a *= b);
    drop(b);
    domain.ifft_in_place(&mut c);
    coset.fft_in_place(&mut c);
    let inv = domain
        .evaluate_vanishing_polynomial(Fr::GENERATOR)
        .inverse()
        .ok_or("证明计算域无效")?;
    for_each_pair(&mut a, &c, |a, c| *a = (*a - c) * inv);
    drop(c);
    coset.ifft_in_place(&mut a);
    let ProvingKey {
        vk,
        beta_g1,
        delta_g1,
        a_query,
        b_g1_query,
        b_g2_query,
        h_query,
        l_query,
    } = pk;
    if h_query.len() + 1 != a.len()
        || !a.last().unwrap().is_zero()
        || a_query.len() != assignment.len()
        || b_g1_query.len() != assignment.len()
        || b_g2_query.len() != assignment.len()
        || l_query.len() + inputs != assignment.len()
    {
        return Err("公开参数维度不匹配".into());
    }
    progress("生成证明 1 / 5…");
    let h = msm::<G1Projective>(&h_query, &a[..h_query.len()]);
    drop(a);
    drop(h_query);
    progress("生成证明 2 / 5…");
    let l = msm::<G1Projective>(&l_query, &assignment[inputs..]);
    drop(l_query);
    progress("生成证明 3 / 5…");
    let ga = msm::<G1Projective>(&a_query, &assignment) + vk.alpha_g1 + delta_g1 * r;
    drop(a_query);
    progress("生成证明 4 / 5…");
    let gb1 = msm::<G1Projective>(&b_g1_query, &assignment) + beta_g1 + delta_g1 * s;
    drop(b_g1_query);
    progress("生成证明 5 / 5…");
    let gb2 = msm::<G2Projective>(&b_g2_query, &assignment) + vk.beta_g2 + vk.delta_g2 * s;
    drop(b_g2_query);
    let gc = ga * s + gb1 * r - delta_g1 * (r * s) + l + h;
    Ok(Proof {
        a: ga.into_affine(),
        b: gb2.into_affine(),
        c: gc.into_affine(),
    })
}

// Keep platform selection out of the proof equations. Both paths apply the
// same operation to disjoint output entries; small jobs stay in coarse chunks.
fn for_each_pair<A: Send, B: Sync>(a: &mut [A], b: &[B], op: impl Fn(&mut A, &B) + Sync + Send) {
    assert_eq!(a.len(), b.len());
    #[cfg(any(not(target_arch = "wasm32"), feature = "web-threads"))]
    let pairs = a.par_iter_mut().zip(b).with_min_len(1024);
    #[cfg(all(target_arch = "wasm32", not(feature = "web-threads")))]
    let pairs = a.iter_mut().zip(b);
    pairs.for_each(|(a, b)| op(a, b));
}

fn first_unsatisfied(a: &[Fr], b: &[Fr], c: &[Fr]) -> Option<usize> {
    let fails = |((a, b), c): ((&Fr, &Fr), &Fr)| *a * b != *c;
    // find-any would make the error depend on thread scheduling.
    #[cfg(any(not(target_arch = "wasm32"), feature = "web-threads"))]
    return a
        .par_iter()
        .zip(b)
        .zip(c)
        .with_min_len(1024)
        .position_first(fails);
    #[cfg(all(target_arch = "wasm32", not(feature = "web-threads")))]
    a.iter().zip(b).zip(c).position(fails)
}
