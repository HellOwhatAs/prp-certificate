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

fn msm<G: CurveGroup<ScalarField = Fr>>(bases: &[G::Affine], scalars: &[Fr]) -> G {
    assert_eq!(bases.len(), scalars.len());
    let mut result = G::zero();
    // Ark's full-size signed-window MSM materializes n*window-count digits.
    // Bound its temporary storage independently of the number of constraints.
    for (points, values) in bases.chunks(65536).zip(scalars.chunks(65536)) {
        let mut p = Vec::with_capacity(points.len());
        let mut s = Vec::with_capacity(points.len());
        for (point, value) in points.iter().zip(values) {
            if !point.is_zero() && !value.is_zero() {
                p.push(*point);
                s.push(value.into_bigint());
            }
        }
        result += G::msm_bigint(&p, &s);
    }
    result
}

pub fn prove(
    pk: ProvingKey<Bn254>,
    b: Builder,
    r: Fr,
    s: Fr,
    mut progress: impl FnMut(&str),
) -> Result<Proof<Bn254>, String> {
    let (assignment, matrices, inputs) = b.into_parts();
    let count = matrices[0].len();
    let domain = GeneralEvaluationDomain::<Fr>::new(count + inputs).ok_or("证明多项式过大")?;
    let size = domain.size();
    let mut polynomials = Vec::with_capacity(3);
    // Drop every matrix row immediately after evaluation; free all sparse
    // matrices before FFT and the large H-query multiplication.
    for rows in matrices {
        let mut values = vec![Fr::zero(); size];
        for (i, row) in rows.into_iter().enumerate() {
            values[i] = evaluate_constraint(&row, &assignment);
        }
        polynomials.push(values);
    }
    let mut iter = polynomials.into_iter();
    let mut a = iter.next().unwrap();
    let mut b = iter.next().unwrap();
    let mut c = iter.next().unwrap();
    drop(iter);
    a[count..count + inputs].copy_from_slice(&assignment[..inputs]);
    progress("计算证明多项式…");
    domain.ifft_in_place(&mut a);
    domain.ifft_in_place(&mut b);
    let coset = domain.get_coset(Fr::GENERATOR).ok_or("证明计算域无效")?;
    coset.fft_in_place(&mut a);
    coset.fft_in_place(&mut b);
    for (a, b) in a.iter_mut().zip(b.iter()) {
        *a *= b;
    }
    drop(b);
    domain.ifft_in_place(&mut c);
    coset.fft_in_place(&mut c);
    let inv = domain
        .evaluate_vanishing_polynomial(Fr::GENERATOR)
        .inverse()
        .ok_or("证明计算域无效")?;
    for (a, c) in a.iter_mut().zip(c.iter()) {
        *a = (*a - c) * inv;
    }
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
