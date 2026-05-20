//! Single-mod install pipeline:
//! resolve instance → fetch ModVersion → cache lookup → cold path →
//! verify SHA-1 → copy into `{instance}/.minecraft/mods/` → record
//! in `{instance}/ftlauncher/installed-mods.json`.

use std::path::Path;

use chrono::Utc;
use sha1::{Digest, Sha1};
use tokio::fs;

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
pub type ProgressFn = Box<dyn Fn(ModInstallPhase, u64, Option<u64>) + Send + Sync>;

/// Coarse-grained phase for a single mod install tick. Named with the
/// `Mod-` prefix at the type level (not just via `#[specta(rename)]`)
/// because the bindings exporter dedupes on the Rust type name and
/// `versions::install::InstallPhase` already owns the short name.
#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ModInstallPhase {
    Downloading,
    Verifying,
    Copying,
}

/// Per-tick IPC payload bundled over a `Channel<ProgressTick>`. Carries
/// the same shape as the internal `ProgressFn` callback, but converted
/// to `f64` (specta forbids BigInt-style `u64` exports). Used by the
/// modpack import command to forward per-mod ticks to the UI without
/// allocating an Event variant per modpack import session.
#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
pub struct ProgressTick {
    pub phase: ModInstallPhase,
    pub current: f64,
    pub total: Option<f64>,
}

/// Ensure the file with content-hash `sha` (lowercase hex) is present in
/// the shared content-addressed cache. Cache hit: returns the cached
/// path immediately. Cache miss: streams the file through the audited
/// chokepoint, SHA-verifies it, and promotes it into the cache. Shared
/// by `install_one` and `install_asset`.
async fn fetch_to_cache(
    data_dir: &Path,
    url: &str,
    sha: &str,
    size: f64,
    initiator: &str,
    progress: &ProgressFn,
) -> Result<std::path::PathBuf, Error> {
    let cached = cache::verify_or_evict(data_dir, sha).await?;
    let cached_path = cache::cache_path_for(data_dir, sha);
    if !cached {
        let tmp = cached_path.with_extension("tmp");
        // download_inner verifies SHA-1 internally, deletes the partial
        // on mismatch, and creates tmp's parent (the cache root).
        crate::network::download::download_inner(url, &tmp, sha, initiator, |dp| {
            progress(
                ModInstallPhase::Downloading,
                dp.bytes_done as u64,
                dp.bytes_total.map(|t| t as u64),
            );
        })
        .await
        .map_err(|e| match e {
            Error::HashMismatch { expected, got, .. } => {
                Error::ModsSha1Mismatch { expected, got }
            }
            Error::Io { path, details } => Error::ModsCacheIo {
                details: format!("{path}: {details}"),
            },
            Error::Network { url, details } => Error::ModsNetwork { url, details },
            other => other,
        })?;
        progress(
            ModInstallPhase::Verifying,
            size as u64,
            Some(size as u64),
        );
        fs::rename(&tmp, &cached_path)
            .await
            .map_err(|e| Error::ModsCacheIo { details: e.to_string() })?;
    }
    Ok(cached_path)
}

pub async fn install_one(
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

    let cached_path = fetch_to_cache(
        data_dir,
        &version.primary_file.url,
        &sha_lower,
        version.primary_file.size,
        "mods",
        progress,
    )
    .await?;

    // 3. Copy into instance
    progress(ModInstallPhase::Copying, 0, None);
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

/// Install a downloaded file to an arbitrary declared path under the
/// instance's `.minecraft/`. Unlike `install_one` this does NOT record
/// anything in `installed-mods.json` — assets (resourcepacks, shaders,
/// configs) are not mods; the modpack import orchestrator tracks them in
/// `pack_origin` instead.
pub async fn install_asset(
    data_dir: &Path,
    instance_root: &Path,
    url: &str,
    sha: &str,
    size: f64,
    install_path: &str,
    progress: &ProgressFn,
) -> Result<(), Error> {
    // String-level guard FIRST — before any directory is created — so an
    // escaping path can never cause a mkdir outside `.minecraft/`.
    if !crate::mods::modpack::path_safety::is_safe_relative_path(install_path) {
        return Err(Error::ModpackOverridesPathEscape {
            entry: install_path.to_string(),
        });
    }
    let sha_lower = sha.to_ascii_lowercase();
    let cached_path =
        fetch_to_cache(data_dir, url, &sha_lower, size, "modpacks", progress).await?;

    progress(ModInstallPhase::Copying, 0, None);
    let mc_dir = instance_root.join(".minecraft");
    let dest = mc_dir.join(install_path);
    let parent = dest.parent().ok_or_else(|| Error::ModsInstancePath {
        path: dest.display().to_string(),
        details: "asset path has no parent directory".into(),
    })?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: parent.display().to_string(),
            details: e.to_string(),
        })?;
    // Defense in depth: the canonical parent must stay inside `.minecraft/`
    // (catches symlink-based escapes the string check cannot see).
    let mc_canon = dunce::canonicalize(&mc_dir).map_err(|e| Error::ModsInstancePath {
        path: mc_dir.display().to_string(),
        details: e.to_string(),
    })?;
    let parent_canon = dunce::canonicalize(parent).map_err(|e| Error::ModsInstancePath {
        path: parent.display().to_string(),
        details: e.to_string(),
    })?;
    if !parent_canon.starts_with(&mc_canon) {
        return Err(Error::ModpackOverridesPathEscape {
            entry: install_path.to_string(),
        });
    }
    fs::copy(&cached_path, &dest)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?;
    Ok(())
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
        let installed = install_one(td_data.path(), td_inst.path(), v, &nop_progress())
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
        install_one(td_data.path(), td_inst.path(), v(), &nop_progress())
            .await
            .unwrap();
        install_one(td_data.path(), td_inst.path(), v(), &nop_progress())
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
        let err = install_one(td_data.path(), td_inst.path(), v, &nop_progress())
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
        let err = install_one(td_data.path(), td_inst.path(), v, &nop_progress())
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
        install_one(td_data.path(), td_inst.path(), v, &nop_progress())
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
    async fn install_asset_writes_to_declared_path() {
        let body = b"resourcepack-bytes";
        let sha = hex::encode(Sha1::digest(body));
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rp.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        install_asset(
            td_data.path(),
            td_inst.path(),
            &format!("{}/rp.zip", s.uri()),
            &sha,
            body.len() as f64,
            "resourcepacks/RP.zip",
            &nop_progress(),
        )
        .await
        .unwrap();
        let dest = td_inst.path().join(".minecraft/resourcepacks/RP.zip");
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), body);
    }

    #[tokio::test]
    async fn install_asset_rejects_path_escape() {
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let r = install_asset(
            td_data.path(),
            td_inst.path(),
            "http://127.0.0.1:1/x",
            "0000000000000000000000000000000000000000",
            1.0,
            "../../escape.zip",
            &nop_progress(),
        )
        .await;
        assert!(
            matches!(r, Err(Error::ModpackOverridesPathEscape { .. })),
            "got: {r:?}"
        );
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
        install_one(td_data.path(), td_inst.path(), v, &nop_progress())
            .await
            .unwrap();
        uninstall(td_inst.path(), &sha).await.unwrap();
        assert!(!installed::mods_dir(td_inst.path()).join("u.jar").exists());
        assert!(cache::cache_path_for(td_data.path(), &sha).exists()); // cache survives
        assert!(installed::list(td_inst.path()).await.unwrap().is_empty());
    }
}
