use crate::{
    checker::{self, Checked},
    groth16::circuit::{self, Witness},
};
use ark_bn254::{Bn254, Fr};
use ark_ff::UniformRand;
use ark_groth16::{Groth16, Proof, VerifyingKey, prepare_verifying_key};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use base64::{Engine, engine::general_purpose::STANDARD};
use flight_format::{Certificate, Metadata, PROOF_BYTES, nonce};
use rand::RngCore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

macro_rules! ensure {($c:expr,$($m:tt)*)=>{if !($c){return Err(format!($($m)*));}};}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn hash(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn manifest() -> &'static Value {
    static M: OnceLock<Value> = OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_slice(include_bytes!("../../../public/groth16/manifest.json"))
            .expect("embedded parameter manifest")
    })
}
fn new_metadata(q: usize, team: &str) -> Result<Metadata, String> {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    let m = Metadata::new(q, team.trim().into(), nonce(&bytes));
    m.validate()?;
    Ok(m)
}
fn public(c: &Certificate) -> Vec<Fr> {
    c.metadata
        .identity()
        .into_iter()
        .map(|b| Fr::from(b as u64))
        .chain(c.scores.iter().map(|x| Fr::from(*x as u64)))
        .collect()
}
fn vk(q: usize) -> Result<VerifyingKey<Bn254>, String> {
    let info = &manifest()["circuits"][q.to_string()];
    let bytes = STANDARD
        .decode(info["vk"].as_str().ok_or("缺少固定校验密钥")?)
        .map_err(err)?;
    ensure!(
        info["vk_sha256"].as_str() == Some(hash(&bytes).as_str()),
        "固定校验密钥损坏"
    );
    VerifyingKey::deserialize_compressed(bytes.as_slice()).map_err(err)
}
pub fn verify_certificate(value: Value, mut progress: impl FnMut(&str)) -> Result<Value, String> {
    let set = flight_format::CertificateSet::from_value(&value)?;
    let mut scores = vec![];
    for c in &set.certificates {
        let result = verify_single(serde_json::to_value(c).map_err(err)?, &mut progress)
            .map_err(|e| format!("第 {} 问：{e}", c.metadata.question))?;
        scores.push(result["scores"][0].clone());
    }
    Ok(json!({"certificate":set,"scores":scores}))
}

fn verify_single(value: Value, mut progress: impl FnMut(&str)) -> Result<Value, String> {
    let c: Certificate = serde_json::from_value(value).map_err(err)?;
    c.metadata.validate()?;
    ensure!(
        (1..=256).contains(&c.scores[3]) && c.scores[6] > 0,
        "成绩范围无效"
    );
    ensure!(c.proof.len() == 172, "证明长度无效");
    let bytes = STANDARD.decode(&c.proof).map_err(|_| "证明编码无效")?;
    ensure!(bytes.len() == PROOF_BYTES, "证明长度无效");
    let proof =
        Proof::<Bn254>::deserialize_compressed(bytes.as_slice()).map_err(|_| "证明点编码无效")?;
    progress("验证 Groth16 成绩证明…");
    let key = prepare_verifying_key(&vk(c.metadata.question)?);
    ensure!(
        Groth16::<Bn254>::verify_proof(&key, &proof, &public(&c)).map_err(err)?,
        "零知识证明验证失败"
    );
    Ok(json!({"scores":[c.scores],"certificate":c}))
}
fn check(job: &Value, q: usize) -> Result<Checked, String> {
    let files = &job["files"];
    checker::check(
        q,
        files["routes"].as_str().ok_or("缺少航线文件")?,
        files["assignments"].as_str().ok_or("缺少人员分配文件")?,
        false,
    )
}
pub fn run_job(
    job: Value,
    _unused: Vec<u8>,
    params: Vec<u8>,
    mut progress: impl FnMut(&str),
) -> Result<Value, String> {
    match job["action"].as_str() {
        Some("verify") => verify_certificate(job["certificate"].clone(), progress),
        Some(action @ ("check" | "prove")) => {
            let q = job["q"]
                .as_u64()
                .filter(|n| (1..=3).contains(n))
                .ok_or("题号无效")? as usize;
            progress("校验方案…");
            let checked = check(&job, q)?;
            if action == "check" {
                return Ok(json!({"scores":[checked.score],"canProve":checked.can_prove()}));
            }
            ensure!(checked.can_prove(), "当前证明上限为 256 架次");
            let m = new_metadata(q, job["team"].as_str().ok_or("队名为空")?)?;
            let id = m.identity();
            let info = &manifest()["circuits"][q.to_string()];
            ensure!(
                info["pk_bytes"].as_u64() == Some(params.len() as u64)
                    && info["pk_sha256"].as_str() == Some(hash(&params).as_str()),
                "公开证明参数完整性检查失败"
            );
            progress("读取固定公开参数…");
            // Bytes are authenticated against the embedded setup manifest BEFORE
            // skipping repeated subgroup checks for these trusted public points.
            let max = info["pk_raw_bytes"].as_u64().ok_or("公开参数尺寸无效")?;
            let pk = crate::parameters::decode_authenticated(&params, max, &mut progress)?;
            drop(params);
            ensure!(pk.vk == vk(q)?, "公开证明参数与固定校验密钥不匹配");
            progress("构建 Groth16 约束…");
            let witness: Witness = serde_json::from_value(checked.groth_witness()?).map_err(err)?;
            let b = circuit::compile(q, &id, &witness)?;
            drop(witness);
            drop(checked);
            ensure!(
                info["matrix"].as_str() == Some(b.digest().as_str()),
                "证明约束与公开参数不一致"
            );
            ensure!(
                pk.a_query.len() == b.assignment.len()
                    && pk.l_query.len() + b.inputs == b.assignment.len(),
                "公开参数维度不匹配"
            );
            let scores: [u32; 9] = std::array::from_fn(|i| job_score(&b.assignment[33 + i]));
            let prepared = crate::groth16::prover::prepare(b)?;
            progress("生成 Groth16 零知识证明…");
            let mut rng = rand::rngs::OsRng;
            let proof = crate::groth16::prover::prove_prepared(
                pk,
                prepared,
                Fr::rand(&mut rng),
                Fr::rand(&mut rng),
                &mut progress,
            )?;
            let mut bytes = vec![];
            proof.serialize_compressed(&mut bytes).map_err(err)?;
            ensure!(bytes.len() == PROOF_BYTES, "证明尺寸异常");
            verify_certificate(
                serde_json::to_value(flight_format::CertificateSet::new(vec![Certificate {
                    metadata: m,
                    proof: STANDARD.encode(bytes),
                    scores,
                }])?)
                .map_err(err)?,
                progress,
            )
        }
        _ => Err("未知操作".into()),
    }
}
fn job_score(n: &Fr) -> u32 {
    use ark_ff::PrimeField;
    n.into_bigint().0[0] as u32
}
