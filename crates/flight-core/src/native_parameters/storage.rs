use super::{Parameters, cache_root, valid_hash};
use serde_json::{Value, json};
use std::fs::{self, File, TryLockError};
use std::path::Path;

pub(super) const BUSY: &str = "参数缓存正在使用，请稍后重试";

// Readers may coexist; deletion must wait until every parameter stream is loaded.
// OS locks are released even when the cancellable engine process is killed.
pub(super) fn lock(root: &Path, exclusive: bool) -> Result<File, String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let path = root.join(".cache.lock");
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err("缓存锁文件无效".into());
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.map_err(|e| match e {
        TryLockError::WouldBlock => BUSY.to_owned(),
        TryLockError::Error(e) => e.to_string(),
    })?;
    Ok(file)
}

fn question(name: &str) -> Option<usize> {
    let (q, index) = name.strip_suffix(".dat")?.split_once('-')?;
    let q = match q {
        "q1" => 1,
        "q2" => 2,
        "q3" => 3,
        _ => return None,
    };
    (!index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())).then_some(q)
}

fn manage_at(
    root: &Path,
    infos: &[Parameters],
    operation: &str,
    q: Option<usize>,
) -> Result<Value, String> {
    if !matches!(operation, "list" | "delete" | "clear")
        || (operation == "delete" && !q.is_some_and(|q| (1..=3).contains(&q)))
    {
        return Err("存储操作无效".into());
    }
    let _lock = if operation == "list" {
        None
    } else {
        Some(lock(root, true)?)
    };
    let mut bytes = [0u64; 3];
    let mut cached = [0u64; 3];
    let mut other = 0u64;
    let entries = match fs::read_dir(root) {
        Ok(entries) => Some(entries),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries.into_iter().flatten() {
        let entry = entry.map_err(|e| e.to_string())?;
        let hash = entry.file_name().to_string_lossy().into_owned();
        if !valid_hash(&hash) || !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        for child in fs::read_dir(entry.path()).map_err(|e| e.to_string())? {
            let child = child.map_err(|e| e.to_string())?;
            if !child.file_type().map_err(|e| e.to_string())?.is_file() {
                continue;
            }
            let name = child.file_name().to_string_lossy().into_owned();
            let part_q = question(&name);
            let temporary = name.starts_with(".part-");
            if part_q.is_none() && !temporary {
                continue;
            }
            let owner = part_q.or_else(|| {
                infos
                    .iter()
                    .position(|i| i.pk_sha256 == hash)
                    .map(|i| i + 1)
            });
            let deleting = operation == "clear" || (operation == "delete" && owner == q);
            if deleting {
                match fs::remove_file(child.path()) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            } else {
                let size = match child.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.to_string()),
                };
                if let Some(q) = owner {
                    bytes[q - 1] += size;
                    if infos.get(q - 1).is_some_and(|info| {
                        info.pk_sha256 == hash
                            && info
                                .chunks
                                .iter()
                                .any(|part| part.file == name && part.bytes as u64 == size)
                    }) {
                        cached[q - 1] += size;
                    }
                } else {
                    other += size;
                }
            }
        }
        // Only remove empty hash directories; unrelated files are never touched.
        if operation != "list" {
            let _ = fs::remove_dir(entry.path());
        }
    }
    Ok(
        json!({"available":true,"rows":bytes.into_iter().enumerate().map(|(i,bytes)|json!({
            "q":i+1,"bytes":bytes,"cached_bytes":cached[i],
            "total_bytes":infos.get(i).map_or(0, |info| info.pk_bytes)
        })).collect::<Vec<_>>(),"other_bytes":other}),
    )
}

/// Inspect or delete downloaded public parameters, never certificates or local bundles.
pub fn manage(operation: &str, q: Option<usize>) -> Result<Value, String> {
    let infos: Vec<Parameters> = (1..=3)
        .map(|q| {
            let info: Parameters = serde_json::from_value(
                super::super::engine::manifest()["circuits"][q.to_string()].clone(),
            )
            .map_err(|_| "证明组件清单无效")?;
            info.validate(q)?;
            Ok(info)
        })
        .collect::<Result<_, String>>()?;
    manage_at(
        &cache_root().ok_or("无法确定参数缓存目录")?,
        &infos,
        operation,
        q,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_progress_requires_all_current_chunks_at_their_expected_size() {
        let (info, chunks) = super::super::tests::fixture();
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join(&info.pk_sha256);
        let old = root.path().join("a".repeat(64));
        fs::create_dir(&current).unwrap();
        fs::create_dir(&old).unwrap();
        fs::write(current.join(&info.chunks[0].file), &chunks[0]).unwrap();
        fs::write(current.join(&info.chunks[1].file), b"truncated").unwrap();
        fs::write(old.join(&info.chunks[1].file), &chunks[1]).unwrap();
        fs::write(current.join(".part-incomplete"), &chunks[1]).unwrap();
        let infos = [info];
        let partial = manage_at(root.path(), &infos, "list", None).unwrap();
        assert_eq!(partial["rows"][0]["cached_bytes"], chunks[0].len());
        assert_eq!(partial["rows"][0]["total_bytes"], infos[0].pk_bytes);
        fs::write(current.join(&infos[0].chunks[1].file), &chunks[1]).unwrap();
        let full = manage_at(root.path(), &infos, "list", None).unwrap();
        assert_eq!(full["rows"][0]["cached_bytes"], infos[0].pk_bytes);
        let empty = manage_at(root.path(), &infos, "delete", Some(1)).unwrap();
        assert_eq!(empty["rows"][0]["cached_bytes"], 0);
        assert_eq!(empty["rows"][0]["bytes"], 0);
    }

    #[test]
    fn deleting_then_loading_downloads_again_and_restores_storage_size() {
        let (info, chunks) = super::super::tests::fixture();
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let outside = root.path().join("public/groth16");
        fs::create_dir_all(&outside).unwrap();
        for (part, bytes) in info.chunks.iter().zip(&chunks) {
            fs::write(outside.join(&part.file), bytes).unwrap();
        }
        let infos = [info];
        for _ in 0..2 {
            let state = manage_at(&cache, &infos, "list", None).unwrap();
            assert_eq!(state["rows"][0]["bytes"], 0);
            let mut downloads = 0;
            let mut messages = vec![];
            let loaded = super::super::load(
                1,
                &infos[0],
                Some(&cache),
                |_| {
                    let bytes = chunks[downloads].clone();
                    downloads += 1;
                    Ok(bytes)
                },
                &mut |message| messages.push(message.to_owned()),
            )
            .unwrap();
            assert_eq!(loaded, chunks.concat());
            assert_eq!(downloads, chunks.len());
            assert!(!messages.iter().any(|message| message.contains("读取")));
            let state = manage_at(&cache, &infos, "list", None).unwrap();
            assert_eq!(state["rows"][0]["bytes"], infos[0].pk_bytes);
            assert_eq!(
                super::super::load(
                    1,
                    &infos[0],
                    Some(&cache),
                    |_| panic!("cached chunks must be reused"),
                    &mut |_| {},
                )
                .unwrap(),
                loaded
            );
            let state = manage_at(&cache, &infos, "delete", Some(1)).unwrap();
            assert_eq!(state["rows"][0]["bytes"], 0);
            assert!(!cache.join(&infos[0].pk_sha256).exists());
        }
        assert_eq!(fs::read_dir(outside).unwrap().count(), chunks.len());
    }

    #[test]
    fn partial_old_and_temporary_files_are_counted_and_cleared_selectively() {
        let (info, _) = super::super::tests::fixture();
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join(&info.pk_sha256);
        let old = root.path().join("a".repeat(64));
        fs::create_dir(&current).unwrap();
        fs::create_dir(&old).unwrap();
        for (dir, name, size) in [
            (&current, "q1-0.dat", 3),
            (&current, ".part-test", 2),
            (&old, "q1-0.dat", 4),
            (&old, "q2-0.dat", 7),
            (&old, ".part-old", 1),
        ] {
            fs::write(dir.join(name), vec![0; size]).unwrap();
        }
        fs::write(current.join("notes.txt"), b"keep").unwrap();
        fs::write(root.path().join("certificate.png"), b"keep").unwrap();
        let infos = [info];
        let state = manage_at(root.path(), &infos, "list", None).unwrap();
        assert_eq!(state["rows"][0]["bytes"], 9);
        assert_eq!(state["rows"][1]["bytes"], 7);
        assert_eq!(state["other_bytes"], 1);
        assert!(old.join("q1-0.dat").exists());
        let state = manage_at(root.path(), &infos, "delete", Some(1)).unwrap();
        assert_eq!(state["rows"][0]["bytes"], 0);
        assert_eq!(state["rows"][1]["bytes"], 7);
        manage_at(root.path(), &infos, "clear", None).unwrap();
        assert!(!old.exists());
        assert!(current.join("notes.txt").exists());
        assert!(root.path().join("certificate.png").exists());
    }

    #[test]
    fn readers_coexist_and_block_deletion_until_released() {
        let root = tempfile::tempdir().unwrap();
        let a = lock(root.path(), false).unwrap();
        let b = lock(root.path(), false).unwrap();
        assert_eq!(lock(root.path(), true).unwrap_err(), BUSY);
        drop((a, b));
        let deletion = lock(root.path(), true).unwrap();
        assert_eq!(lock(root.path(), false).unwrap_err(), BUSY);
        drop(deletion);
        assert!(lock(root.path(), false).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn deletion_does_not_follow_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("q1-0.dat"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("a".repeat(64))).unwrap();
        manage_at(root.path(), &[], "clear", None).unwrap();
        assert_eq!(fs::read(outside.path().join("q1-0.dat")).unwrap(), b"keep");
    }
}
