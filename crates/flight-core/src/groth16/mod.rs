pub mod circuit;
mod permutation;
pub mod prover;
pub mod r1cs;

#[cfg(test)]
mod tests {
    use super::r1cs::{Builder, Num};
    use ark_bn254::{Bn254, Fr};
    use ark_ff::UniformRand;
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
}
