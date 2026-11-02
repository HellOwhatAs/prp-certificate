//! Canonical public certificate schema; independent of proving and image code.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub const FORMAT: &str = "prp-flight-groth16/1";
pub const RULES: &str = "prp-flight-final-g16-v1";
pub const DATASET: &str = include_str!("../../../public/data/dataset-id.txt");
pub const PROOF_BYTES: usize = 128;
pub const MAX_PAYLOAD: usize = 346;
macro_rules! ensure {($c:expr,$($m:tt)*)=>{if !($c){return Err(format!($($m)*));}};}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub format: String,
    pub rules: String,
    pub dataset: String,
    pub question: usize,
    pub team: String,
    pub nonce: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    pub metadata: Metadata,
    pub proof: String,
    pub scores: [u32; 9],
}
impl Metadata {
    pub fn new(q: usize, team: String, nonce: String) -> Self {
        Self {
            format: FORMAT.into(),
            rules: RULES.into(),
            dataset: DATASET.into(),
            question: q,
            team,
            nonce,
        }
    }
    pub fn identity(&self) -> [u8; 32] {
        Sha256::digest(
            serde_json::to_vec(&json!([
                self.format,
                self.rules,
                self.dataset,
                self.question,
                self.team,
                self.nonce
            ]))
            .unwrap(),
        )
        .into()
    }
    pub fn validate(&self) -> Result<(), String> {
        ensure!(
            self.format == FORMAT && self.rules == RULES && self.dataset == DATASET,
            "证书规则或证明格式不受支持，请用原始解答重新生成"
        );
        ensure!((1..=3).contains(&self.question), "题号无效");
        ensure!(
            (1..=40).contains(&self.team.chars().count())
                && self.team == self.team.trim()
                && !self.team.chars().any(char::is_control),
            "队名格式无效"
        );
        let n = self.nonce.as_bytes();
        ensure!(
            n.len() == 36
                && n.iter()
                    .enumerate()
                    .all(|(i, b)| if [8, 13, 18, 23].contains(&i) {
                        *b == b'-'
                    } else {
                        b.is_ascii_digit() || (b'a'..=b'f').contains(b)
                    }),
            "证书编号无效"
        );
        Ok(())
    }
}
pub fn nonce(bytes: &[u8]) -> String {
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
/// Magic4, question1, team-byte-length1, nonce16, scores36, proof128, team.
pub fn pack(value: &Value) -> Result<Vec<u8>, String> {
    let c: Certificate = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    c.metadata.validate()?;
    let proof = STANDARD.decode(&c.proof).map_err(|_| "证明编码无效")?;
    ensure!(proof.len() == PROOF_BYTES, "证明长度无效");
    let team = c.metadata.team.as_bytes();
    ensure!(team.len() <= 160, "队名过长");
    let mut out = b"PRG1".to_vec();
    out.extend([c.metadata.question as u8, team.len() as u8]);
    let h = c.metadata.nonce.replace('-', "");
    for i in (0..32).step_by(2) {
        out.push(u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| "证书编号无效")?);
    }
    for s in c.scores {
        out.extend(s.to_be_bytes());
    }
    out.extend(proof);
    out.extend(team);
    Ok(out)
}
/// Decoding does not authenticate claims: always run the fixed-key verifier.
pub fn unpack(bytes: &[u8]) -> Result<Value, String> {
    ensure!(
        bytes.len() >= 186 && bytes.len() <= MAX_PAYLOAD && &bytes[..4] == b"PRG1",
        "不是当前 Groth16 证书"
    );
    let team_len = bytes[5] as usize;
    ensure!(bytes.len() == 186 + team_len, "证书载荷长度无效");
    let team = std::str::from_utf8(&bytes[186..])
        .map_err(|_| "队名编码无效")?
        .to_owned();
    let m = Metadata::new(bytes[4] as usize, team, nonce(&bytes[6..22]));
    m.validate()?;
    let scores = std::array::from_fn(|i| {
        u32::from_be_bytes(bytes[22 + i * 4..26 + i * 4].try_into().unwrap())
    });
    let c = Certificate {
        metadata: m,
        scores,
        proof: STANDARD.encode(&bytes[58..186]),
    };
    serde_json::to_value(c).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_binary_roundtrip_and_length_guards() {
        let c = Certificate {
            metadata: Metadata::new(3, "🚁".repeat(40), nonce(&[17; 16])),
            proof: STANDARD.encode([42; 128]),
            scores: [u32::MAX; 9],
        };
        let value = serde_json::to_value(c).unwrap();
        let b = pack(&value).unwrap();
        assert_eq!(b.len(), 346);
        assert_eq!(unpack(&b).unwrap(), value);
        for n in 0..b.len() {
            assert!(unpack(&b[..n]).is_err());
        }
        let mut extra = b.clone();
        extra.push(0);
        assert!(unpack(&extra).is_err());
        let mut bad = b;
        bad[4] = 4;
        assert!(unpack(&bad).is_err());
    }
}
