pub mod circuit;
mod permutation;
pub mod prover;
pub mod r1cs;

#[cfg(test)]
mod tests {
    use super::r1cs::{Builder, Num};
    use ark_bn254::{Bn254, Fr};
    use ark_ff::{Field, UniformRand};
    use ark_groth16::{Groth16, prepare_verifying_key};
    use ark_relations::gr1cs::{ConstraintSynthesizer, ConstraintSystem};
    use ark_serialize::CanonicalSerialize;
    fn tiny() -> Builder {
        let mut b = Builder::new(&[23, 12]);
        b.range(b.public(0), 8);
        let x = b.uint(3);
        let y = b.uint(4);
        let z = b.mul(x, y);
        b.eq(z, b.public(1));
        b.eq(Num::c(1), Num::c(1));
        // Exercise duplicate wires, cancellation, constants and non-unit field
        // coefficients against arkworks' independent prover implementation.
        let wide = b.lin(&[
            (x, -Fr::ONE),
            (y, Fr::from(7)),
            (x, Fr::from(9)),
            (y, -Fr::from(7)),
            (Num::c(2), Fr::from(5)),
        ]);
        b.eq(wide, Num::c(34));
        b
    }
    #[test]
    fn ark_matrix_roundtrip_proof_and_identity_binding() {
        let b = tiny();
        let expected = b.matrices.clone();
        let cs = ConstraintSystem::new_ref();
        b.generate_constraints(cs.clone()).unwrap();
        cs.finalize();
        assert!(cs.is_satisfied().unwrap());
        assert_eq!(cs.to_matrices().unwrap()["R1CS"], expected);
        let mut rng = rand::rngs::OsRng;
        let pk =
            Groth16::<Bn254>::generate_random_parameters_with_reduction(tiny(), &mut rng).unwrap();
        let b = tiny();
        let r = Fr::rand(&mut rng);
        let s = Fr::rand(&mut rng);
        let proof = Groth16::<Bn254>::create_proof_with_reduction_and_matrices(
            &pk,
            r,
            s,
            &b.matrices,
            b.inputs,
            b.matrices[0].len(),
            &b.assignment,
        )
        .unwrap();
        let bounded = super::prover::prove(pk.clone(), tiny(), r, s, |_| {}).unwrap();
        assert_eq!(proof, bounded, "bounded prover must match upstream exactly");
        assert_eq!(proof.compressed_size(), 128);
        let vk = prepare_verifying_key(&pk.vk);
        let mut public = b.assignment[1..b.inputs].to_vec();
        assert!(Groth16::<Bn254>::verify_proof(&vk, &proof, &public).unwrap());
        public[0] += Fr::from(1);
        assert!(!Groth16::<Bn254>::verify_proof(&vk, &proof, &public).unwrap());
        public[0] -= Fr::from(1);
        public[1] += Fr::from(1);
        assert!(!Groth16::<Bn254>::verify_proof(&vk, &proof, &public).unwrap());
    }

    #[test]
    fn preparation_rejects_first_unsatisfied_constraint_across_chunks() {
        let make = || {
            let mut b = Builder::new(&[7]);
            for i in 0..9000 {
                let x = b.uint(if i == 2048 || i == 8192 { 8 } else { 7 });
                b.eq(x, b.public(0));
            }
            b
        };
        let expected = "R1CS constraint 2048 failed";
        assert_eq!(make().satisfied().unwrap_err(), expected);
        assert_eq!(super::prover::prepare(make()).err().unwrap(), expected);
    }

    #[test]
    #[ignore = "full-sized Q1/Q2/Q3 matrices; run explicitly in release mode"]
    fn all_circuit_matrices_match_pinned_setup() {
        let manifest: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../public/groth16/manifest.json"))
                .unwrap();
        for q in 1..=3 {
            let b =
                super::circuit::compile(q, &[0; 32], &super::circuit::Witness::blank(q)).unwrap();
            assert_eq!(
                b.digest(),
                manifest["circuits"][q.to_string()]["matrix"]
                    .as_str()
                    .unwrap(),
                "Q{q}"
            );
            assert_eq!(
                b.stats(),
                manifest["circuits"][q.to_string()]["stats"],
                "Q{q}"
            );
        }
    }
}
