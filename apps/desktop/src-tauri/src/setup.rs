use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::Emitter;
pub const MANIFEST: &str = include_str!("../../../../resources/setup-manifest.json");
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub license: String,
    pub license_url: String,
    pub kind: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub version: String,
    pub assets: Vec<Asset>,
}
pub fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST).expect("bundled trusted manifest")
}
pub fn selected(root: &Path) -> Manifest {
    let p = crate::preferences::load(root);
    let mut m = manifest();
    m.assets.retain(|a| {
        a.id == "runtime"
            || a.id == p.model
            || a.id == "embedding"
            || (a.id == "runtime-vulkan" && p.backend == "vulkan")
    });
    m
}
pub fn asset_dir(root: &Path, a: &Asset) -> PathBuf {
    root.join(if a.kind == "runtime" {
        "runtime"
    } else {
        "models"
    })
    .join(format!("{}-{}", a.id, a.version))
}
pub fn model_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    let a = manifest()
        .assets
        .into_iter()
        .find(|a| a.id == id)
        .ok_or("unknown asset")?;
    let p = asset_dir(root, &a).join("model.gguf");
    if !p.is_file() {
        return Err("モデル未導入。セットアップを実行してください。".into());
    }
    Ok(p)
}
pub fn runtime_path(root: &Path) -> Result<PathBuf, String> {
    runtime_path_for(root, "runtime")
}
pub fn runtime_path_for(root: &Path, id: &str) -> Result<PathBuf, String> {
    let a = manifest().assets.into_iter().find(|a| a.id == id).unwrap();
    let dir = asset_dir(root, &a);
    let mut archive =
        zip::ZipArchive::new(File::open(dir.join("runtime.zip")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| e.to_string())?;
        let rel = f.enclosed_name().ok_or("invalid archive path")?;
        if rel
            .file_name()
            .is_some_and(|name| name == "llama-server.exe")
        {
            return Ok(dir.join(rel));
        }
    }
    Err("ランタイム未導入".into())
}
fn find_server(dir: &Path) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(p) = find_server(&p) {
                return Some(p);
            }
        } else if p.file_name()?.to_str()? == "llama-server.exe" {
            return Some(p);
        }
    }
    None
}
pub fn hash_file(p: &Path) -> Result<String, String> {
    let mut f = File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut b = [0; 1024 * 1024];
    loop {
        let n = f.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
pub fn verify(p: &Path, a: &Asset) -> Result<(), String> {
    if fs::metadata(p).map_err(|e| e.to_string())?.len() != a.size || hash_file(p)? != a.sha256 {
        return Err(format!(
            "{}: サイズまたはSHA-256不一致。修復を再実行してください。",
            a.id
        ));
    }
    Ok(())
}
pub fn available(root: &Path) -> u64 {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| root.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(0)
}
pub fn install(
    root: &Path,
    cancel: &AtomicBool,
    emit: impl Fn(&str, u64, u64),
) -> Result<(), String> {
    let m = selected(root);
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    for a in &m.assets {
        if cancel.load(Ordering::SeqCst) {
            return Err("中断しました。部分ダウンロードから再開できます。".into());
        }
        let dest = asset_dir(root, a);
        if dest.join("verified.json").is_file() {
            if verify_installed(root, a).is_ok() {
                emit(&a.id, a.size, a.size);
                continue;
            }
        }
        let stage = root.join("staging");
        fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        let partial = stage.join(format!("{}.part", a.sha256));
        let mut offset = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        if offset > a.size {
            fs::remove_file(&partial).map_err(|e| e.to_string())?;
            offset = 0;
        }
        if available(root) < a.size.saturating_sub(offset) + a.size + 256 * 1024 * 1024 {
            return Err(
                "空き容量不足です。ダウンロードと展開用の空き容量を確保してください。".into(),
            );
        }
        if offset < a.size {
            crate::download::fetch(a, &partial, offset, cancel, &emit)?;
        }
        if let Err(e) = verify(&partial, a) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        let unpack = stage.join(format!("{}-unpack", a.sha256));
        if unpack.exists() {
            fs::remove_dir_all(&unpack).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
        if a.kind == "runtime" {
            extract_zip(&partial, &unpack)?;
            if find_server(&unpack).is_none() {
                return Err("llama-server.exe が含まれていません".into());
            }
            fs::copy(&partial, unpack.join("runtime.zip")).map_err(|e| e.to_string())?;
        } else {
            fs::copy(&partial, unpack.join("model.gguf")).map_err(|e| e.to_string())?;
        }
        let mut files = serde_json::Map::new();
        record_files(&unpack, &unpack, &mut files)?;
        fs::write(
            unpack.join("verified.json"),
            serde_json::to_vec(&files).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        // Same-version repair moves the old directory aside until verified activation succeeds.
        let backup = dest.with_extension("previous");
        if backup.exists() {
            fs::remove_dir_all(&backup).map_err(|e| e.to_string())?;
        }
        if dest.exists() {
            fs::rename(&dest, &backup).map_err(|e| e.to_string())?;
        }
        if let Err(e) = fs::rename(&unpack, &dest) {
            if backup.exists() {
                let _ = fs::rename(&backup, &dest);
            }
            return Err(e.to_string());
        }
        let _ = fs::remove_file(partial);
        emit(&a.id, a.size, a.size);
    }
    Ok(())
}
fn record_files(
    root: &Path,
    dir: &Path,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_dir() {
            record_files(root, &p, out)?;
        } else {
            out.insert(
                p.strip_prefix(root).unwrap().to_string_lossy().into(),
                serde_json::json!(hash_file(&p)?),
            );
        }
    }
    Ok(())
}
pub fn verify_installed(root: &Path, a: &Asset) -> Result<(), String> {
    let dir = asset_dir(root, a);
    if a.kind == "model" {
        return verify(&dir.join("model.gguf"), a);
    }
    let archive = dir.join("runtime.zip");
    verify(&archive, a)?;
    let mut archive = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let rel = f.enclosed_name().ok_or("invalid archive path")?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if hash_file(&dir.join(rel))? != format!("{:x}", Sha256::digest(&bytes)) {
            return Err("runtime file corrupted".into());
        }
    }
    Ok(())
}
fn extract_zip(p: &Path, dest: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(File::open(p).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let rel = f.enclosed_name().ok_or("archive traversal")?;
        if f.is_symlink() {
            return Err("archive symlink".into());
        }
        let out = dest.join(rel);
        total += f.size();
        if total > 1024 * 1024 * 1024 {
            return Err("archive too large".into());
        }
        if f.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut File::create(out).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn event(app: &tauri::AppHandle, id: &str, done: u64, total: u64) {
    let _ = app.emit(
        "setup-progress",
        serde_json::json!({"id":id,"done":done,"total":total}),
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_rejects_corruption() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("test");
        fs::write(&p, b"abc").unwrap();
        let a = Asset {
            id: "test".into(),
            version: "1".into(),
            url: "https://example.invalid".into(),
            size: 3,
            sha256: hash_file(&p).unwrap(),
            license: "MIT".into(),
            license_url: "https://example.invalid".into(),
            kind: "model".into(),
        };
        verify(&p, &a).unwrap();
        fs::write(&p, b"abd").unwrap();
        assert!(verify(&p, &a).is_err());
    }
    #[test]
    fn manifest_pinned() {
        for a in manifest().assets {
            assert_eq!(a.sha256.len(), 64);
            assert!(a.size > 0);
            assert!(a.url.starts_with("https://"));
            assert!(!a.url.contains("/main/"));
        }
    }
}
