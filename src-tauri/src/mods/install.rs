//! Single-mod install pipeline:
//! resolve instance → fetch ModVersion → cache lookup → cold path →
//! verify SHA-1 → copy into `{instance}/.minecraft/mods/` → record
//! in `{instance}/ftlauncher/installed-mods.json`.

use std::path::Path;

use chrono::Utc;
use futures_util::StreamExt;
use reqwest::Client;
use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::Error;
use crate::mods::cache;
use crate::mods::installed;
use crate::mods::platform::*;

/// Outcome of installing one mod. Callers can chain events off these.
#[derive(Debug)]
pub struct Installed {
    pub sha1: String,
    pub filename: String,
    pub name: String,
}

/// Progress emitter — caller supplies the function that turns a
/// progress tick into a Tauri event. Lets us unit-test the pipeline
/// without depending on `tauri::AppHandle`.
pub type ProgressFn = Box<dyn Fn(InstallPhase, u64, Option<u64>) + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub enum InstallPhase {
    Downloading,
    Verifying,
    Copying,
}

pub async fn install_one(
    http: &Client,
    data_dir: &Path,
    instance_root: &Path,
    version: ModVersion,
    progress: &ProgressFn,
) -> Result<Installed, Error> {
    if !version.primary_file.distribution_allowed {
        return Err(Error::ModsDistributionDisabled {
            platform: match version.source {
                ModSource::Modrinth => "modrinth",
                ModSource::Curseforge => "curseforge",
            }
            .into(),
            project_id: version.project_id.clone(),
        });
    }
    let sha = version
        .primary_file
        .sha1
        .as_ref()
        .cloned()
        .ok_or(Error::ModsSha1Unavailable)?;
    let sha_lower = sha.to_ascii_lowercase();

    // 1. Cache lookup
    let cached = cache::verify_or_evict(data_dir, &sha_lower).await?;
    let cached_path = cache::cache_path_for(data_dir, &sha_lower);
    if !cached {
        // 2. Cold download
        progress(
            InstallPhase::Downloading,
            0,
            Some(version.primary_file.size as u64),
        );
        let resp = http
            .get(&version.primary_file.url)
            .send()
            .await
            .map_err(|e| Error::ModsNetwork {
                url: version.primary_file.url.clone(),
                details: e.to_string(),
            })?;
        if !resp.status().is_success() {
            return Err(Error::ModsNetwork {
                url: version.primary_file.url.clone(),
                details: format!("HTTP {}", resp.status()),
            });
        }
        let tmp = cached_path.with_extension("jar.tmp");
        fs::create_dir_all(cache::cache_root(data_dir))
            .await
            .map_err(|e| Error::ModsCacheIo { details: e.to_string() })?;
        let mut f = fs::File::create(&tmp)
            .await
            .map_err(|e| Error::ModsCacheIo { details: e.to_string() })?;
        let mut hasher = Sha1::new();
        let mut total_written: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| Error::ModsNetwork {
                url: version.primary_file.url.clone(),
                details: e.to_string(),
            })?;
            hasher.update(&bytes);
            f.write_all(&bytes)
                .await
                .map_err(|e| Error::ModsCacheIo { details: e.to_string() })?;
            total_written += bytes.len() as u64;
            progress(
                InstallPhase::Downloading,
                total_written,
                Some(version.primary_file.size as u64),
            );
        }
        f.flush()
            .await
            .map_err(|e| Error::ModsCacheIo { details: e.to_string() })?;
        drop(f);
        progress(InstallPhase::Verifying, total_written, Some(total_written));
        let got = hex::encode(hasher.finalize());
        if !got.eq_ignore_ascii_case(&sha_lower) {
            fs::remove_file(&tmp).await.ok();
            return Err(Error::ModsSha1Mismatch {
                expected: sha_lower,
                got,
            });
        }
        fs::rename(&tmp, &cached_path)
            .await
            .map_err(|e| Error::ModsCacheIo { details: e.to_string() })?;
    }

    // 3. Copy into instance
    progress(InstallPhase::Copying, 0, None);
    let dest_dir = installed::mods_dir(instance_root);
    fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = dest_dir.join(&version.primary_file.filename);
    if fs::try_exists(&dest)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?
    {
        let existing_bytes = fs::read(&dest)
            .await
            .map_err(|e| Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })?;
        let existing_sha = hex::encode(Sha1::digest(&existing_bytes));
        if existing_sha.eq_ignore_ascii_case(&sha_lower) {
            // Idempotent re-install: same content already in place. Record + return.
        } else {
            return Err(Error::ModsFilenameConflict {
                filename: version.primary_file.filename.clone(),
                existing_sha,
                incoming_sha: sha_lower,
            });
        }
    } else {
        fs::copy(&cached_path, &dest)
            .await
            .map_err(|e| Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })?;
    }

    // 4. Record
    installed::add(
        instance_root,
        InstalledMod {
            filename: version.primary_file.filename.clone(),
            sha1: sha_lower.clone(),
            source: Some(version.source),
            project_id: Some(version.project_id.clone()),
            version_id: Some(version.version_id.clone()),
            name: version.name.clone(),
            version_number: Some(version.version_number.clone()),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        },
    )
    .await?;

    Ok(Installed {
        sha1: sha_lower,
        filename: version.primary_file.filename,
        name: version.name,
    })
}

/// Disable: rename `.jar` → `.jar.disabled` and flip JSON flag.
pub async fn disable(instance_root: &Path, sha1: &str) -> Result<(), Error> {
    flip_enabled(instance_root, sha1, false).await
}

pub async fn enable(instance_root: &Path, sha1: &str) -> Result<(), Error> {
    flip_enabled(instance_root, sha1, true).await
}

async fn flip_enabled(instance_root: &Path, sha1: &str, enable: bool) -> Result<(), Error> {
    let dir = installed::mods_dir(instance_root);
    let mods = installed::list(instance_root).await?;
    let target = mods
        .iter()
        .find(|m| m.sha1.eq_ignore_ascii_case(sha1))
        .ok_or_else(|| Error::ModsNotFound {
            platform: "installed".into(),
        })?;
    let current_name = if target.enabled {
        target.filename.clone()
    } else {
        format!("{}.disabled", target.filename)
    };
    let desired_name = if enable {
        target.filename.clone()
    } else {
        format!("{}.disabled", target.filename)
    };
    if current_name != desired_name {
        let from = dir.join(&current_name);
        let to = dir.join(&desired_name);
        fs::rename(&from, &to)
            .await
            .map_err(|e| Error::ModsInstancePath {
                path: from.display().to_string(),
                details: e.to_string(),
            })?;
    }
    installed::set_enabled(instance_root, sha1, enable).await
}

pub async fn uninstall(instance_root: &Path, sha1: &str) -> Result<(), Error> {
    let dir = installed::mods_dir(instance_root);
    let mods = installed::list(instance_root).await?;
    if let Some(target) = mods.iter().find(|m| m.sha1.eq_ignore_ascii_case(sha1)) {
        let candidate = dir.join(&target.filename);
        let disabled = dir.join(format!("{}.disabled", target.filename));
        for p in [&candidate, &disabled] {
            if fs::try_exists(p)
                .await
                .map_err(|e| Error::ModsInstancePath {
                    path: p.display().to_string(),
                    details: e.to_string(),
                })?
            {
                fs::remove_file(p)
                    .await
                    .map_err(|e| Error::ModsInstancePath {
                        path: p.display().to_string(),
                        details: e.to_string(),
                    })?;
                break;
            }
        }
    }
    installed::remove(instance_root, sha1).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn nop_progress() -> ProgressFn {
        Box::new(|_, _, _| {})
    }

    fn fake_version(url: String, sha: String, bytes_len: u64, filename: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "p".into(),
            version_id: "v".into(),
            name: "Mod".into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: filename.into(),
                url,
                sha1: Some(sha),
                size: bytes_len as f64,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[tokio::test]
    async fn cold_download_populates_cache_and_installs() {
        let s = MockServer::start().await;
        let payload = b"hello-mod-bytes";
        let sha = hex::encode(Sha1::digest(payload));
        Mock::given(method("GET"))
            .and(path("/x.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&s)
            .await;

        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let v = fake_version(
            format!("{}/x.jar", s.uri()),
            sha.clone(),
            payload.len() as u64,
            "x.jar",
        );
        let installed = install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v,
            &nop_progress(),
        )
        .await
        .unwrap();
        assert_eq!(installed.sha1, sha);
        assert!(installed::mods_dir(td_inst.path()).join("x.jar").exists());
        assert!(cache::cache_path_for(td_data.path(), &sha).exists());
        let list = installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source, Some(ModSource::Modrinth));
    }

    #[tokio::test]
    async fn idempotent_reinstall_succeeds() {
        let s = MockServer::start().await;
        let payload = b"abc";
        let sha = hex::encode(Sha1::digest(payload));
        Mock::given(method("GET"))
            .and(path("/y.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .expect(1) // second install should hit cache, not network
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let v = || fake_version(format!("{}/y.jar", s.uri()), sha.clone(), 3, "y.jar");
        install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v(),
            &nop_progress(),
        )
        .await
        .unwrap();
        install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v(),
            &nop_progress(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn filename_conflict_with_different_sha_errors() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/z.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"second".to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let dir = installed::mods_dir(td_inst.path());
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join("z.jar"), b"first").await.unwrap(); // pre-existing different bytes
        let sha = hex::encode(Sha1::digest(b"second"));
        let v = fake_version(format!("{}/z.jar", s.uri()), sha, 6, "z.jar");
        let err = install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v,
            &nop_progress(),
        )
        .await
        .unwrap_err();
        matches!(err, Error::ModsFilenameConflict { .. });
    }

    #[tokio::test]
    async fn distribution_disabled_short_circuits() {
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let mut v = fake_version("https://example/x.jar".into(), "a".into(), 0, "x.jar");
        v.primary_file.distribution_allowed = false;
        let err = install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v,
            &nop_progress(),
        )
        .await
        .unwrap_err();
        matches!(err, Error::ModsDistributionDisabled { .. });
    }

    #[tokio::test]
    async fn disable_then_enable_round_trip() {
        let s = MockServer::start().await;
        let payload = b"dd";
        let sha = hex::encode(Sha1::digest(payload));
        Mock::given(method("GET"))
            .and(path("/d.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let v = fake_version(format!("{}/d.jar", s.uri()), sha.clone(), 2, "d.jar");
        install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v,
            &nop_progress(),
        )
        .await
        .unwrap();
        disable(td_inst.path(), &sha).await.unwrap();
        assert!(installed::mods_dir(td_inst.path())
            .join("d.jar.disabled")
            .exists());
        assert!(!installed::mods_dir(td_inst.path()).join("d.jar").exists());
        enable(td_inst.path(), &sha).await.unwrap();
        assert!(installed::mods_dir(td_inst.path()).join("d.jar").exists());
    }

    #[tokio::test]
    async fn uninstall_removes_file_and_record_but_keeps_cache() {
        let s = MockServer::start().await;
        let payload = b"uu";
        let sha = hex::encode(Sha1::digest(payload));
        Mock::given(method("GET"))
            .and(path("/u.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let v = fake_version(format!("{}/u.jar", s.uri()), sha.clone(), 2, "u.jar");
        install_one(
            &Client::new(),
            td_data.path(),
            td_inst.path(),
            v,
            &nop_progress(),
        )
        .await
        .unwrap();
        uninstall(td_inst.path(), &sha).await.unwrap();
        assert!(!installed::mods_dir(td_inst.path()).join("u.jar").exists());
        assert!(cache::cache_path_for(td_data.path(), &sha).exists()); // cache survives
        assert!(installed::list(td_inst.path()).await.unwrap().is_empty());
    }
}
