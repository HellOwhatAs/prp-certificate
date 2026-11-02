//! Canonical public certificate schema; independent of proving and image code.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub const FORMAT: &str = "prp-flight-groth16/1";
pub const RULES: &str = "prp-flight-final-g16-v1";
pub const DATASET: &str = include_str!("../../../public/data/dataset-id.txt");
pub const PROOF_BYTES: usize = 128;
pub const BUNDLE_FORMAT: &str = "prp-flight-certificate-set/1";
pub const MAX_BUNDLE_PAYLOAD: usize = 706;
macro_rules! ensure {($c:expr,$($m:tt)*)=>{if !($c){return Err(format!($($m)*));}};}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub format: String,
    pub rules: String,
    pub dataset: String,
    pub question: usize,
    pub team: String,
    pub nonce: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    pub metadata: Metadata,
    pub proof: String,
    pub scores: [u32; 9],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificateSet {
    pub format: String,
    pub certificates: Vec<Certificate>,
}

impl CertificateSet {
    pub fn new(mut certificates: Vec<Certificate>) -> Result<Self, String> {
        certificates.sort_by_key(|c| c.metadata.question);
        let set = Self {
            format: BUNDLE_FORMAT.into(),
            certificates,
        };
        set.validate()?;
        Ok(set)
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        let set: Self = serde_json::from_value(value.clone()).map_err(|_| "证书格式无效")?;
        set.validate()?;
        Ok(set)
    }

    pub fn validate(&self) -> Result<(), String> {
        ensure!(self.format == BUNDLE_FORMAT, "证书格式不受支持");
        ensure!(
            (1..=3).contains(&self.certificates.len()),
            "证书须包含一至三问"
        );
        let mut previous = 0;
        for c in &self.certificates {
            c.metadata.validate()?;
            ensure!(c.metadata.question > previous, "证书题号重复或顺序无效");
            ensure!(
                c.metadata.team == self.certificates[0].metadata.team,
                "各问队名不一致"
            );
            let proof = STANDARD.decode(&c.proof).map_err(|_| "证明编码无效")?;
            ensure!(proof.len() == PROOF_BYTES, "证明长度无效");
            previous = c.metadata.question;
        }
        Ok(())
    }

    pub fn team(&self) -> &str {
        &self.certificates[0].metadata.team
    }

    pub fn id(&self) -> Result<String, String> {
        Ok(Sha256::digest(pack_set(self)?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}

/// PRG2: magic4, question mask1, shared team byte length1, team,
/// then (nonce16, scores36, proof128) for each present question in order.
/// Each original proof still authenticates its own question, team and claims.
pub fn pack_set(set: &CertificateSet) -> Result<Vec<u8>, String> {
    set.validate()?;
    let team = set.team().as_bytes();
    ensure!(team.len() <= 160, "队名过长");
    let mask = set
        .certificates
        .iter()
        .fold(0u8, |mask, c| mask | (1 << (c.metadata.question - 1)));
    let mut bytes = b"PRG2".to_vec();
    bytes.extend([mask, team.len() as u8]);
    bytes.extend(team);
    for c in &set.certificates {
        let hex = c.metadata.nonce.replace('-', "");
        for i in (0..32).step_by(2) {
            bytes.push(u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "证书编号无效")?);
        }
        for score in c.scores {
            bytes.extend(score.to_be_bytes());
        }
        bytes.extend(STANDARD.decode(&c.proof).map_err(|_| "证明编码无效")?);
    }
    Ok(bytes)
}

pub fn unpack_set(bytes: &[u8]) -> Result<CertificateSet, String> {
    ensure!(
        bytes.len() >= 6 && bytes.len() <= MAX_BUNDLE_PAYLOAD && bytes.starts_with(b"PRG2"),
        "不是当前证书"
    );
    let mask = bytes[4];
    ensure!(mask > 0 && mask <= 7, "证书题号无效");
    let team_len = bytes[5] as usize;
    ensure!(
        team_len <= 160 && bytes.len() == 6 + team_len + mask.count_ones() as usize * 180,
        "证书载荷长度无效"
    );
    let team = std::str::from_utf8(&bytes[6..6 + team_len]).map_err(|_| "队名编码无效")?;
    let mut at = 6 + team_len;
    let mut certificates = vec![];
    for q in 1..=3 {
        if mask & (1 << (q - 1)) == 0 {
            continue;
        }
        certificates.push(Certificate {
            metadata: Metadata::new(q, team.to_owned(), nonce(&bytes[at..at + 16])),
            scores: std::array::from_fn(|i| {
                u32::from_be_bytes(bytes[at + 16 + i * 4..at + 20 + i * 4].try_into().unwrap())
            }),
            proof: STANDARD.encode(&bytes[at + 52..at + 180]),
        });
        at += 180;
    }
    CertificateSet::new(certificates)
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
#[cfg(test)]
mod bundle_tests {
    use super::*;
    fn entry(q: usize, team: &str) -> Certificate {
        Certificate {
            metadata: Metadata::new(q, team.into(), nonce(&[q as u8; 16])),
            scores: [u32::MAX; 9],
            proof: STANDARD.encode([q as u8; 128]),
        }
    }
    #[test]
    fn all_subsets_have_stable_question_positions() {
        for mask in 1..=7 {
            let entries = (1..=3)
                .filter(|q| mask & (1 << (q - 1)) != 0)
                .map(|q| entry(q, "测试队伍"))
                .collect();
            let set = CertificateSet::new(entries).unwrap();
            let bytes = pack_set(&set).unwrap();
            assert_eq!(bytes[4], mask);
            assert_eq!(unpack_set(&bytes).unwrap(), set);
            assert_eq!(
                CertificateSet::from_value(&serde_json::to_value(&set).unwrap()).unwrap(),
                set
            );
        }
    }
    #[test]
    fn max_length_and_malformed_bundles() {
        assert!(
            CertificateSet::from_value(&serde_json::to_value(entry(1, "测试")).unwrap()).is_err()
        );
        assert!(unpack_set(b"PRG1").is_err());
        let set =
            CertificateSet::new((1..=3).map(|q| entry(q, &"🚁".repeat(40))).collect()).unwrap();
        let bytes = pack_set(&set).unwrap();
        assert_eq!(bytes.len(), MAX_BUNDLE_PAYLOAD);
        assert_eq!(unpack_set(&bytes).unwrap(), set);
        for n in 0..bytes.len() {
            assert!(unpack_set(&bytes[..n]).is_err());
        }
        let mut bad = bytes.clone();
        bad.push(0);
        assert!(unpack_set(&bad).is_err());
        for mask in [0, 8, 255] {
            let mut bad = bytes.clone();
            bad[4] = mask;
            assert!(unpack_set(&bad).is_err());
        }
        assert!(CertificateSet::new(vec![]).is_err());
        assert!(CertificateSet::new(vec![entry(1, "甲"), entry(1, "甲")]).is_err());
        assert!(CertificateSet::new(vec![entry(1, "甲"), entry(2, "乙")]).is_err());
        let mut bad = set.clone();
        bad.certificates.swap(0, 1);
        assert!(bad.validate().is_err());
        let mut value = serde_json::to_value(set).unwrap();
        value["extra"] = json!(1);
        assert!(CertificateSet::from_value(&value).is_err());
    }
}
