//! One-time native setup. Only public parameters are written; RNG seeds and
//! trapdoor scalars remain in this short-lived process and are never logged.
use ark_bn254::Bn254;
use ark_groth16::Groth16;
use ark_serialize::CanonicalSerialize;
use flight_core::groth16::circuit::{self, Witness};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
fn hash(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn main() {
    let q: usize = std::env::args()
        .nth(1)
        .expect("question 1..3")
        .parse()
        .unwrap();
    let out = std::path::PathBuf::from("public/groth16");
    std::fs::create_dir_all(&out).unwrap();
    assert!(
        !out.join(format!("q{q}.vk")).exists(),
        "setup already exists; use a new parameter directory for a deliberate rotation"
    );
    let start = std::time::Instant::now();
    let b = circuit::compile(q, &[0; 32], &Witness::blank(q)).unwrap();
    let stats = b.stats();
    let digest = b.digest();
    eprintln!("Q{q}: {stats}; matrix {digest}; generating parameters");
    let pk = Groth16::<Bn254>::generate_random_parameters_with_reduction(b, &mut rand::rngs::OsRng)
        .unwrap();
    eprintln!(
        "Q{q}: setup {:.1}s; serializing public key",
        start.elapsed().as_secs_f64()
    );
    let mut vk = vec![];
    pk.vk.serialize_compressed(&mut vk).unwrap();
    let raw_size = pk.compressed_size();
    let path = std::path::PathBuf::from(format!(".cache/groth16/q{q}.pk.zlib"));
    let file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let mut file = flate2::write::ZlibEncoder::new(file, flate2::Compression::new(3));
    pk.serialize_compressed(&mut file).unwrap();
    let mut file = file.finish().unwrap();
    file.flush().unwrap();
    drop(file);
    drop(pk);
    let mut file = std::fs::File::open(&path).unwrap();
    let mut whole = Sha256::new();
    let mut chunks = vec![];
    let mut size = 0usize;
    loop {
        let mut block = vec![0; 4_194_304];
        let mut n = 0;
        while n < block.len() {
            let k = file.read(&mut block[n..]).unwrap();
            if k == 0 {
                break;
            }
            n += k;
        }
        if n == 0 {
            break;
        }
        block.truncate(n);
        whole.update(&block);
        size += n;
        let name = format!("q{q}-{}.dat", chunks.len());
        std::fs::write(out.join(&name), &block).unwrap();
        chunks.push(serde_json::json!({"file":name,"bytes":n,"sha256":hash(&block)}));
    }
    std::fs::write(out.join(format!("q{q}.vk")), &vk).unwrap();
    let info = serde_json::json!({"question":q,"matrix":digest,"stats":stats,"pk_bytes":size,"pk_raw_bytes":raw_size,"encoding":"zlib+ark-compressed","pk_sha256":whole.finalize().iter().map(|b|format!("{b:02x}")).collect::<String>(),"vk_sha256":hash(&vk),"chunks":chunks,"setup_seconds":start.elapsed().as_secs_f64()});
    std::fs::write(
        out.join(format!("q{q}.json")),
        serde_json::to_vec_pretty(&info).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Q{q}: finished; public key {size} bytes; {:.1}s",
        start.elapsed().as_secs_f64()
    );
}
