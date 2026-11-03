//! Decode the pinned arkworks parameter encoding in bounded batches.
//!
//! The caller MUST authenticate the complete compressed byte stream against the
//! embedded setup manifest first. As in the upstream unchecked reader, subgroup
//! checks are omitted only for these fixed, authenticated public parameters.
use ark_bn254::{Bn254, G1Affine, G2Affine};
use ark_groth16::{ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, SerializationError};
use std::io::{Read, Take};

#[cfg(any(not(target_arch = "wasm32"), feature = "web-threads"))]
use rayon::prelude::*;

const BATCH_POINTS: usize = 32_768;

fn points<T, R>(reader: &mut Take<R>) -> Result<Vec<T>, SerializationError>
where
    T: CanonicalDeserialize + CanonicalSerialize + Default + Send,
    R: Read,
{
    let count = u64::deserialize_compressed(&mut *reader)?;
    let size = T::default().compressed_size();
    let bytes = count
        .checked_mul(size as u64)
        .ok_or(SerializationError::InvalidData)?;
    // One extra byte is reserved for detecting an overlong decompressed stream.
    if bytes > reader.limit().saturating_sub(1) {
        return Err(SerializationError::InvalidData);
    }
    let count = usize::try_from(count).map_err(|_| SerializationError::InvalidData)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| SerializationError::InvalidData)?;
    let mut buffer = vec![0u8; count.min(BATCH_POINTS) * size];
    while result.len() < count {
        let n = (count - result.len()).min(BATCH_POINTS);
        let bytes = &mut buffer[..n * size];
        reader.read_exact(bytes)?;
        #[cfg(any(not(target_arch = "wasm32"), feature = "web-threads"))]
        let chunks = bytes.par_chunks_exact(size);
        #[cfg(all(target_arch = "wasm32", not(feature = "web-threads")))]
        let chunks = bytes.chunks_exact(size);
        let batch: Result<Vec<T>, _> = chunks.map(T::deserialize_compressed_unchecked).collect();
        result.extend(batch?);
    }
    Ok(result)
}

pub(crate) fn decode_authenticated(
    bytes: &[u8],
    raw_bytes: u64,
    progress: &mut impl FnMut(&str),
) -> Result<ProvingKey<Bn254>, String> {
    fn decode(
        bytes: &[u8],
        raw_bytes: u64,
        progress: &mut impl FnMut(&str),
    ) -> Result<ProvingKey<Bn254>, SerializationError> {
        let limit = raw_bytes
            .checked_add(1)
            .ok_or(SerializationError::InvalidData)?;
        let mut reader = flate2::read::ZlibDecoder::new(bytes).take(limit);
        // Field order is the ark-groth16 0.6 canonical ProvingKey encoding.
        let vk = VerifyingKey {
            alpha_g1: G1Affine::deserialize_compressed_unchecked(&mut reader)?,
            beta_g2: G2Affine::deserialize_compressed_unchecked(&mut reader)?,
            gamma_g2: G2Affine::deserialize_compressed_unchecked(&mut reader)?,
            delta_g2: G2Affine::deserialize_compressed_unchecked(&mut reader)?,
            gamma_abc_g1: points(&mut reader)?,
        };
        let beta_g1 = G1Affine::deserialize_compressed_unchecked(&mut reader)?;
        let delta_g1 = G1Affine::deserialize_compressed_unchecked(&mut reader)?;
        progress("读取公开参数 1 / 5…");
        let a_query = points(&mut reader)?;
        progress("读取公开参数 2 / 5…");
        let b_g1_query = points(&mut reader)?;
        progress("读取公开参数 3 / 5…");
        let b_g2_query = points(&mut reader)?;
        progress("读取公开参数 4 / 5…");
        let h_query = points(&mut reader)?;
        progress("读取公开参数 5 / 5…");
        let l_query = points(&mut reader)?;
        let mut extra = [0u8; 1];
        if reader.read(&mut extra)? != 0 || reader.limit() != 1 {
            return Err(SerializationError::InvalidData);
        }
        Ok(ProvingKey {
            vk,
            beta_g1,
            delta_g1,
            a_query,
            b_g1_query,
            b_g2_query,
            h_query,
            l_query,
        })
    }
    decode(bytes, raw_bytes, progress).map_err(|e| format!("公开参数读取失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ec::AffineRepr;
    use std::io::Write;

    fn compress(raw: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn matches_upstream_encoding_across_batches() {
        let g1 = G1Affine::generator();
        let g2 = G2Affine::generator();
        let points = [g1, -g1, G1Affine::identity()];
        let key = ProvingKey {
            vk: VerifyingKey {
                alpha_g1: g1,
                beta_g2: g2,
                gamma_g2: -g2,
                delta_g2: g2,
                gamma_abc_g1: points.to_vec(),
            },
            beta_g1: -g1,
            delta_g1: g1,
            a_query: (0..BATCH_POINTS + 3).map(|i| points[i % 3]).collect(),
            b_g1_query: points.to_vec(),
            b_g2_query: vec![g2, -g2, G2Affine::identity()],
            h_query: vec![],
            l_query: points.to_vec(),
        };
        let mut raw = vec![];
        key.serialize_compressed(&mut raw).unwrap();
        let compressed = compress(&raw);
        let decoded = decode_authenticated(&compressed, raw.len() as u64, &mut |_| {}).unwrap();
        assert_eq!(decoded, key);
        assert_eq!(
            decoded,
            ProvingKey::deserialize_compressed_unchecked(raw.as_slice()).unwrap()
        );
        for size in [raw.len() as u64 - 1, raw.len() as u64 + 1] {
            assert!(decode_authenticated(&compressed, size, &mut |_| {}).is_err());
        }
        assert!(
            decode_authenticated(
                &compressed[..compressed.len() / 2],
                raw.len() as u64,
                &mut |_| {}
            )
            .is_err()
        );
        let last = raw.len() - 1;
        raw[last] = 0xff;
        assert!(decode_authenticated(&compress(&raw), raw.len() as u64, &mut |_| {}).is_err());
    }

    #[test]
    fn rejects_oversized_vector_before_allocation() {
        let bytes = u64::MAX.to_le_bytes();
        let mut reader = bytes.as_slice().take(9);
        assert!(points::<G1Affine, _>(&mut reader).is_err());
    }
}
