//! Fetch only authenticated public proving components. No solution data leaves the machine.
mod storage;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
pub use storage::manage;

#[derive(Clone, Deserialize)]
struct Part {
    file: String,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize)]
struct Parameters {
    pk_bytes: usize,
    pk_sha256: String,
    chunks: Vec<Part>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Parameters {
    fn validate(&self, q: u64) -> Result<(), String> {
        let mut names = HashSet::new();
        let mut total = 0usize;
        if !(1..=3).contains(&q)
            || self.pk_bytes == 0
            || self.pk_bytes > 600_000_000
            || !valid_hash(&self.pk_sha256)
            || self.chunks.is_empty()
            || self.chunks.len() > 200
        {
            return Err("证明组件清单无效".into());
        }
        for part in &self.chunks {
            let number = part
                .file
                .strip_prefix(&format!("q{q}-"))
                .and_then(|s| s.strip_suffix(".dat"));
            if !number.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                || !names.insert(&part.file)
                || part.bytes == 0
                || part.bytes > 4_194_304
                || !valid_hash(&part.sha256)
            {
                return Err("证明组件清单无效".into());
            }
            total += part.bytes;
        }
        if total != self.pk_bytes {
            return Err("证明组件长度无效".into());
        }
        Ok(())
    }
}
impl Part {
    fn valid(&self, bytes: &[u8]) -> bool {
        bytes.len() == self.bytes && digest(bytes) == self.sha256
    }
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() != self.bytes as u64 {
            return None;
        }
        let mut bytes = Vec::with_capacity(self.bytes);
        file.take(self.bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        self.valid(&bytes).then_some(bytes)
    }
}

fn cache_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FLIGHT_CACHE_DIR").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Caches"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")));
    base.map(|p| p.join("prp-flight").join("parameters"))
}
// Only a complete, hash-checked chunk is committed. Parallel app instances may
// download the same chunk; they never observe each other's partial writes.
fn cache_part(directory: &Path, part: &Part, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let destination = directory.join(&part.file);
    if destination.exists() {
        if part.read(&destination).is_some() {
            return Ok(());
        }
        match std::fs::remove_file(&destination) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut pending = tempfile::Builder::new()
        .prefix(".part-")
        .tempfile_in(directory)
        .map_err(|e| e.to_string())?;
    pending.write_all(bytes).map_err(|e| e.to_string())?;
    pending.as_file().sync_all().map_err(|e| e.to_string())?;
    match pending.persist_noclobber(&destination) {
        Ok(_) => Ok(()),
        Err(_) if part.read(&destination).is_some() => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn download(agent: &ureq::Agent, url: &str, part: &Part) -> Result<Vec<u8>, String> {
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("下载失败：{e}"))?;
    if response.status() != 200 {
        return Err("下载响应无效".into());
    }
    if let Some(length) = response.header("Content-Length") {
        if length.parse::<usize>().ok() != Some(part.bytes) {
            return Err("下载长度无效".into());
        }
    }
    let mut bytes = Vec::with_capacity(part.bytes);
    response
        .into_reader()
        .take(part.bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("下载中断：{e}"))?;
    if !part.valid(&bytes) {
        return Err("下载的证明组件校验失败".into());
    }
    Ok(bytes)
}

fn load(
    q: u64,
    info: &Parameters,
    cache: Option<&Path>,
    mut fetch: impl FnMut(&Part) -> Result<Vec<u8>, String>,
    progress: &mut impl FnMut(&str),
) -> Result<Vec<u8>, String> {
    info.validate(q)?;
    let mut out = Vec::new();
    out.try_reserve_exact(info.pk_bytes)
        .map_err(|_| "内存不足，无法加载证明组件")?;
    let directory = cache.map(|p| p.join(&info.pk_sha256));
    let mut cache_writable = true;
    progress(&format!(
        "准备证明组件（首次约 {} MB）…",
        info.pk_bytes.div_ceil(1_000_000)
    ));
    for part in &info.chunks {
        let saved = directory
            .as_ref()
            .and_then(|p| part.read(&p.join(&part.file)));
        let downloaded = saved.is_none();
        let bytes = if let Some(bytes) = saved {
            bytes
        } else {
            progress(&format!(
                "下载证明组件 {} / {} MB…",
                out.len() / 1_000_000,
                info.pk_bytes.div_ceil(1_000_000)
            ));
            let bytes =
                fetch(part).map_err(|e| format!("{e}。请检查网络后重试，已缓存的分片会保留"))?;
            if !part.valid(&bytes) {
                return Err("证明组件校验失败".into());
            }
            if cache_writable {
                if let Some(path) = &directory {
                    if cache_part(path, part, &bytes).is_err() {
                        cache_writable = false;
                        progress("无法保存参数缓存，本次下载仍可继续");
                    }
                }
            }
            bytes
        };
        out.extend_from_slice(&bytes);
        progress(&format!(
            "{}证明组件 {} / {} MB…",
            if downloaded { "下载" } else { "读取缓存" },
            out.len().div_ceil(1_000_000),
            info.pk_bytes.div_ceil(1_000_000)
        ));
    }
    if digest(&out) != info.pk_sha256 {
        return Err("完整证明组件校验失败".into());
    }
    Ok(out)
}

pub(crate) fn read(q: u64, progress: &mut impl FnMut(&str)) -> Result<Vec<u8>, String> {
    let mut root = cache_root();
    let _lock = match root
        .as_deref()
        .map(|root| storage::lock(root, false))
        .transpose()
    {
        Ok(lock) => lock,
        Err(error) if error == storage::BUSY => return Err(error),
        Err(_) => {
            root = None;
            progress("无法保存参数缓存，本次下载仍可继续");
            None
        }
    };
    let info: Parameters =
        serde_json::from_value(super::engine::manifest()["circuits"][q.to_string()].clone())
            .map_err(|_| "证明组件清单无效")?;
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(45))
        .timeout_write(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .redirects(3)
        .build();
    load(
        q,
        &info,
        root.as_deref(),
        |part| {
            let url = format!("{}{}", env!("FLIGHT_PARAMETER_BASE_URL"), part.file);
            let mut last = String::new();
            for attempt in 0..3 {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_millis(250 * attempt));
                }
                match download(&agent, &url, part) {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) => last = error,
                }
            }
            Err(last)
        },
        progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "downloads one 4 MiB public component from GitHub"]
    fn github_download_smoke() {
        let info: Parameters =
            serde_json::from_value(super::super::engine::manifest()["circuits"]["1"].clone())
                .unwrap();
        let part = &info.chunks[0];
        let agent = ureq::AgentBuilder::new()
            .https_only(true)
            .try_proxy_from_env(true)
            .timeout(Duration::from_secs(30))
            .build();
        let url = format!("{}{}", env!("FLIGHT_PARAMETER_BASE_URL"), part.file);
        assert!(part.valid(&download(&agent, &url, part).unwrap()));
    }
    pub(super) fn fixture() -> (Parameters, Vec<Vec<u8>>) {
        let bytes = vec![
            b"first public component".to_vec(),
            b"second public component".to_vec(),
        ];
        let all = bytes.concat();
        let info = Parameters {
            pk_bytes: all.len(),
            pk_sha256: digest(&all),
            chunks: bytes
                .iter()
                .enumerate()
                .map(|(i, b)| Part {
                    file: format!("q1-{i}.dat"),
                    bytes: b.len(),
                    sha256: digest(b),
                })
                .collect(),
        };
        (info, bytes)
    }
    #[test]
    fn download_then_reuse_cache_without_network() {
        let (info, bytes) = fixture();
        let cache = tempfile::tempdir().unwrap();
        let mut calls = 0;
        let all = load(
            1,
            &info,
            Some(cache.path()),
            |_| {
                let value = bytes[calls].clone();
                calls += 1;
                Ok(value)
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(all, bytes.concat());
        assert_eq!(
            load(
                1,
                &info,
                Some(cache.path()),
                |_| panic!("unexpected network"),
                &mut |_| {}
            )
            .unwrap(),
            all
        );
    }
    #[test]
    fn interruption_preserves_completed_chunks_and_repairs_corrupt_cache() {
        let (info, bytes) = fixture();
        let cache = tempfile::tempdir().unwrap();
        let directory = cache.path().join(&info.pk_sha256);
        let mut calls = 0;
        assert!(
            load(
                1,
                &info,
                Some(cache.path()),
                |_| {
                    calls += 1;
                    if calls == 1 {
                        Ok(bytes[0].clone())
                    } else {
                        Err("interrupted".into())
                    }
                },
                &mut |_| {}
            )
            .is_err()
        );
        assert!(directory.join("q1-0.dat").is_file());
        assert!(!directory.join("q1-1.dat").exists());
        let all = load(
            1,
            &info,
            Some(cache.path()),
            |part| {
                assert_eq!(part.file, "q1-1.dat");
                Ok(bytes[1].clone())
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(all, bytes.concat());
        std::fs::write(directory.join("q1-0.dat"), vec![0; bytes[0].len()]).unwrap();
        assert_eq!(
            load(
                1,
                &info,
                Some(cache.path()),
                |part| {
                    assert_eq!(part.file, "q1-0.dat");
                    Ok(bytes[0].clone())
                },
                &mut |_| {}
            )
            .unwrap(),
            all
        );
        assert!(info.chunks[0].read(&directory.join("q1-0.dat")).is_some());
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), 2);
    }
    #[test]
    fn forged_or_malformed_parameters_are_not_accepted() {
        let (mut info, bytes) = fixture();
        let cache = tempfile::tempdir().unwrap();
        assert!(
            load(
                1,
                &info,
                Some(cache.path()),
                |p| Ok(vec![0; p.bytes]),
                &mut |_| {}
            )
            .is_err()
        );
        assert!(!cache.path().join(&info.pk_sha256).exists());
        info.chunks[0].file = "../q1-0.dat".into();
        assert!(
            load(
                1,
                &info,
                None,
                |_| panic!("unexpected network"),
                &mut |_| {}
            )
            .is_err()
        );
        info.chunks[0].file = "q1-0.dat".into();
        info.pk_sha256 = "0".repeat(64);
        let mut next = 0;
        assert!(
            load(
                1,
                &info,
                None,
                |_| {
                    let b = bytes[next].clone();
                    next += 1;
                    Ok(b)
                },
                &mut |_| {}
            )
            .is_err()
        );
    }
    #[test]
    fn unwritable_cache_does_not_prevent_generation() {
        let (info, bytes) = fixture();
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("not-a-directory");
        std::fs::write(&cache, b"occupied").unwrap();
        let mut next = 0;
        let mut messages = vec![];
        assert_eq!(
            load(
                1,
                &info,
                Some(&cache),
                |_| {
                    let b = bytes[next].clone();
                    next += 1;
                    Ok(b)
                },
                &mut |s| messages.push(s.to_owned())
            )
            .unwrap(),
            bytes.concat()
        );
        assert!(messages.iter().any(|s| s.contains("无法保存参数缓存")));
    }
    #[test]
    fn http_transport_rejects_truncation_overflow_and_http_errors() {
        use std::net::TcpListener;
        let (info, bytes) = fixture();
        let part = &info.chunks[0];
        for (status, body, length, accepted) in [
            (200, bytes[0].clone(), Some(part.bytes), true),
            (200, bytes[0][..3].to_vec(), Some(part.bytes), false),
            (200, [bytes[0].clone(), vec![1]].concat(), None, false),
            (200, vec![0; part.bytes], Some(part.bytes), false),
            (206, bytes[0].clone(), Some(part.bytes), false),
            (503, vec![], Some(0), false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 2048];
                let _ = stream.read(&mut request);
                let length = length
                    .map(|n| format!("Content-Length: {n}\r\n"))
                    .unwrap_or_default();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nConnection: close\r\n{length}\r\n"
                );
                let _ = stream.write_all(&body);
            });
            let agent = ureq::AgentBuilder::new()
                .try_proxy_from_env(false)
                .timeout(Duration::from_secs(2))
                .build();
            assert_eq!(
                download(&agent, &format!("http://{address}/q1-0.dat"), part).is_ok(),
                accepted
            );
            server.join().unwrap();
        }
    }
}
