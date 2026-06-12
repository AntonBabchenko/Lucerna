//! Single-mod install pipeline:
//! resolve instance → fetch ModVersion → cache lookup → cold path →
//! verify SHA-1 → copy into `{instance}/.minecraft/mods/` → record
//! in `{instance}/lucerna/installed-mods.json`.

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

/// Outcome of `update_one`: the new primary install, the required-
/// dependency installs that ran, and the SHA-1 of the old jar removed.
#[derive(Debug)]
pub struct UpdateOutcome {
    pub primary: Installed,
    pub deps: Vec<Installed>,
    pub removed_sha1: String,
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
pub(crate) async fn fetch_to_cache(
    data_dir: &Path,
    url: &str,
    sha: &str,
    size: f64,
    initiator: &str,
    progress: &ProgressFn,
) -> Result<std::path::PathBuf, Error> {
    // No-TOFU (Principle B.6): refuse to install a file whose integrity we cannot
    // verify, before any I/O. Mirrors install_asset's guard so the invariant holds
    // at the sink, not only on the summary-formation path. An empty expected hash
    // would otherwise skip verification in download_inner.
    if sha.trim().is_empty() {
        return Err(Error::ModsSha1Unavailable);
    }
    let cached = cache::verify_or_evict(data_dir, sha).await?;
    let cached_path = cache::cache_path_for(data_dir, sha);
    if !cached {
        let tmp = cached_path.with_extension("tmp");
        // download_inner verifies SHA-1 internally, deletes the partial
        // on mismatch, and creates tmp's parent (the cache root).
        crate::network::download::download_inner(
            url,
            &tmp,
            crate::network::download::Checksum::Sha1(sha.to_string()),
            initiator,
            |dp| {
                progress(
                    ModInstallPhase::Downloading,
                    dp.bytes_done as u64,
                    dp.bytes_total.map(|t| t as u64),
                );
            },
        )
        .await
        .map_err(|e| match e {
            Error::HashMismatch { expected, got, .. } => Error::ModsSha1Mismatch { expected, got },
            Error::Io { path, details } => Error::ModsCacheIo {
                details: format!("{path}: {details}"),
            },
            Error::Network { url, details } => Error::ModsNetwork { url, details },
            other => other,
        })?;
        progress(ModInstallPhase::Verifying, size as u64, Some(size as u64));
        fs::rename(&tmp, &cached_path)
            .await
            .map_err(|e| Error::ModsCacheIo {
                details: e.to_string(),
            })?;
    }
    Ok(cached_path)
}

/// Download an md5-only file (ATLauncher server/direct mods), verifying the
/// supplied md5, then promote it into the sha1-keyed content cache under the
/// sha1 computed over the same bytes. Returns `(cache_path, computed_sha1)`.
///
/// Unlike `fetch_to_cache`, the cache key is not known up front — it is the
/// sha1 the download returns. So this always downloads to a temp, verifies
/// md5, then renames into the cache under the computed sha1 (a no-op cost if
/// the sha1 already happens to be cached, which is fine).
pub(crate) async fn fetch_to_cache_md5(
    data_dir: &Path,
    url: &str,
    md5_hex: &str,
    size: f64,
    initiator: &str,
    progress: &ProgressFn,
) -> Result<(std::path::PathBuf, String), Error> {
    let tmp = cache::cache_path_for(data_dir, md5_hex).with_extension("md5tmp");
    let computed_sha1 = crate::network::download::download_inner(
        url,
        &tmp,
        crate::network::download::Checksum::Md5(md5_hex.to_ascii_lowercase()),
        initiator,
        |dp| {
            progress(
                ModInstallPhase::Downloading,
                dp.bytes_done as u64,
                dp.bytes_total.map(|t| t as u64),
            );
        },
    )
    .await
    .map_err(|e| match e {
        Error::HashMismatch { expected, got, .. } => Error::ModsSha1Mismatch { expected, got },
        Error::Io { path, details } => Error::ModsCacheIo {
            details: format!("{path}: {details}"),
        },
        Error::Network { url, details } => Error::ModsNetwork { url, details },
        other => other,
    })?;

    progress(ModInstallPhase::Verifying, size as u64, Some(size as u64));
    let cached_path = cache::cache_path_for(data_dir, &computed_sha1);
    fs::rename(&tmp, &cached_path)
        .await
        .map_err(|e| Error::ModsCacheIo {
            details: e.to_string(),
        })?;
    Ok((cached_path, computed_sha1))
}

pub async fn install_one(
    data_dir: &Path,
    instance_root: &Path,
    version: ModVersion,
    progress: &ProgressFn,
) -> Result<Installed, Error> {
    // Guard FIRST — before any network or filesystem I/O — that the
    // platform-supplied filename is a safe single segment. `filename` comes
    // straight from the Modrinth/CurseForge API; a value like `../../evil.jar`
    // would otherwise `join` outside the instance `mods/` directory.
    if !crate::mods::modpack::path_safety::is_safe_filename(&version.primary_file.filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: version.primary_file.filename.clone(),
        });
    }

    if !version.primary_file.distribution_allowed {
        return Err(Error::ModsDistributionDisabled {
            platform: match version.source {
                ModSource::Modrinth => "modrinth",
                ModSource::Curseforge => "curseforge",
                ModSource::Ftb => "ftb", // FTB: pack-managed, not individually distributable.
                ModSource::Atlauncher => "atlauncher", // ATLauncher: pack-managed, not individually distributable.
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
        let existing_bytes = fs::read(&dest).await.map_err(|e| Error::ModsInstancePath {
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
            enrich_attempted: false,
            requires: Vec::new(),
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
    // Empty/whitespace sha1 guard FIRST (no-TOFU, Principle B.6): refuse to
    // install a file whose integrity we cannot verify, before any I/O.
    if sha.trim().is_empty() {
        return Err(Error::ModsSha1Unavailable);
    }
    // String-level guard FIRST — before any directory is created — so an
    // escaping path can never cause a mkdir outside `.minecraft/`.
    if !crate::mods::modpack::path_safety::is_safe_relative_path(install_path) {
        return Err(Error::ModpackOverridesPathEscape {
            entry: install_path.to_string(),
        });
    }
    let sha_lower = sha.to_ascii_lowercase();
    let cached_path = fetch_to_cache(data_dir, url, &sha_lower, size, "modpacks", progress).await?;

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

/// Directory under `.minecraft/` for a non-mod content kind.
fn asset_dir(kind: crate::mods::platform::ContentKind) -> &'static str {
    use crate::mods::platform::ContentKind::*;
    match kind {
        ResourcePack => "resourcepacks",
        Shader => "shaderpacks",
        Mod => "mods", // assets path is rp/shader only; never used for Mod
    }
}

/// `<asset_dir>/<filename>` — the install path under `.minecraft/`. Shared by
/// `install_asset_tracked` and the uninstall command (Task 7).
pub fn asset_subpath(kind: crate::mods::platform::ContentKind, filename: &str) -> String {
    format!("{}/{}", asset_dir(kind), filename)
}

/// Inverse of `asset_dir`: map an install path to the asset kind it lands in,
/// or `None` for non-asset paths (`mods/`, `config/`, `scripts/`, …). Used by
/// the modpack importer to decide which staged files also belong in the
/// per-instance assets registry.
pub fn content_kind_for_install_path(
    install_path: &str,
) -> Option<crate::mods::platform::ContentKind> {
    use crate::mods::platform::ContentKind;
    if install_path.starts_with("resourcepacks/") {
        Some(ContentKind::ResourcePack)
    } else if install_path.starts_with("shaderpacks/") {
        Some(ContentKind::Shader)
    } else {
        None
    }
}

/// Resolve the on-disk path for an asset removal, rejecting any filename that
/// would escape the instance's `.minecraft/` directory.
///
/// Defense-in-depth: `install_asset` validates the same way on the install
/// path, so a registry basename always passes.  This guard closes the
/// asymmetry on the uninstall path.
///
/// We validate both the raw `filename` and the composed relative path
/// (`<asset_dir>/<filename>`).  Validating the filename alone catches
/// absolute paths and traversals that `asset_subpath` would otherwise
/// silently embed inside the composed string.
pub fn safe_asset_remove_path(
    instance_root: &std::path::Path,
    kind: crate::mods::platform::ContentKind,
    filename: &str,
) -> Result<std::path::PathBuf, Error> {
    // Validate the bare filename first — catches `/abs`, `..` in the name,
    // backslashes, empty string, etc.
    if !crate::mods::modpack::path_safety::is_safe_relative_path(filename) {
        return Err(Error::ModpackOverridesPathEscape {
            entry: filename.to_string(),
        });
    }
    // Then validate the full relative path as a belt-and-suspenders check.
    let rel = asset_subpath(kind, filename);
    if !crate::mods::modpack::path_safety::is_safe_relative_path(&rel) {
        return Err(Error::ModpackOverridesPathEscape {
            entry: filename.to_string(),
        });
    }
    let mc_dir = instance_root.join(".minecraft");
    let dest = mc_dir.join(&rel);
    // Defense in depth (mirrors install_asset): the canonical parent must stay
    // inside `.minecraft/`, catching symlink-based escapes the string checks
    // cannot see (e.g. a symlink under shaderpacks/ redirecting the delete out
    // of the instance). Only canonicalize when the parent actually exists — if
    // the asset dir is absent there is nothing to remove and remove_file will
    // no-op, so an absent dir must not be treated as an escape.
    if let Some(parent) = dest.parent() {
        if parent.exists() {
            let mc_canon = dunce::canonicalize(&mc_dir).map_err(|e| Error::ModsInstancePath {
                path: mc_dir.display().to_string(),
                details: e.to_string(),
            })?;
            let parent_canon =
                dunce::canonicalize(parent).map_err(|e| Error::ModsInstancePath {
                    path: parent.display().to_string(),
                    details: e.to_string(),
                })?;
            if !parent_canon.starts_with(&mc_canon) {
                return Err(Error::ModpackOverridesPathEscape {
                    entry: filename.to_string(),
                });
            }
        }
    }
    Ok(dest)
}

/// Download + install a resource pack or shader, then record it in the
/// per-instance assets registry. Routes by `kind`; never touches installed-mods.json.
#[allow(clippy::too_many_arguments)]
pub async fn install_asset_tracked(
    data_dir: &Path,
    instance_root: &Path,
    kind: crate::mods::platform::ContentKind,
    source: Option<ModSource>,
    project_id: Option<String>,
    version_id: Option<String>,
    name: &str,
    version_number: Option<String>,
    filename: &str,
    url: &str,
    sha: Option<&str>,
    size: f64,
    progress: &ProgressFn,
) -> Result<(), Error> {
    // No-TOFU: refuse before any download/IO when the platform omits a SHA-1.
    // Mirrors `install_one`'s `ok_or(Error::ModsSha1Unavailable)?` so an asset
    // is never written without an integrity check (CurseForge can omit SHA-1).
    let sha = sha.ok_or(Error::ModsSha1Unavailable)?;
    let install_path = asset_subpath(kind, filename);
    install_asset(
        data_dir,
        instance_root,
        url,
        sha,
        size,
        &install_path,
        progress,
    )
    .await?;
    crate::mods::assets::add(
        instance_root,
        crate::mods::platform::InstalledAsset {
            kind,
            filename: filename.to_string(),
            sha1: sha.to_ascii_lowercase(),
            source,
            project_id,
            version_id,
            name: name.to_string(),
            version_number,
            installed_at: Utc::now().to_rfc3339(),
        },
    )
    .await
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

/// Update one installed mod to `target`, installing `target`'s required
/// dependencies (`required_deps`) and removing the old jar (`old_sha1`).
/// The new install preserves the old mod's enabled/disabled state.
///
/// Order is "warm the cache, then swap": every download (target + deps)
/// is fetched into the shared cache FIRST, so a network failure aborts
/// before the instance is touched. Only then is the old jar removed and
/// the new files installed from the warm cache. Mirrors the two-phase
/// shape of `modpack_apply_update`.
pub async fn update_one(
    data_dir: &Path,
    instance_root: &Path,
    old_sha1: &str,
    target: ModVersion,
    required_deps: Vec<ModVersion>,
    progress: &ProgressFn,
) -> Result<UpdateOutcome, Error> {
    // Remember the old mod's enabled state before anything is removed.
    let was_enabled = installed::list(instance_root)
        .await?
        .iter()
        .find(|m| m.sha1.eq_ignore_ascii_case(old_sha1))
        .map(|m| m.enabled)
        .unwrap_or(true);

    // Phase 1 — warm the cache. Filename- and distribution-check then fetch
    // each file; nothing on the instance is touched, so any failure aborts
    // cleanly. The filename guard runs before `fetch_to_cache` so a hostile
    // API filename is rejected before any network I/O — mirroring the
    // guard-first ordering in `install_one`.
    for v in std::iter::once(&target).chain(required_deps.iter()) {
        if !crate::mods::modpack::path_safety::is_safe_filename(&v.primary_file.filename) {
            return Err(Error::ModsUnsafeFilename {
                filename: v.primary_file.filename.clone(),
            });
        }
        if !v.primary_file.distribution_allowed {
            return Err(Error::ModsDistributionDisabled {
                platform: match v.source {
                    ModSource::Modrinth => "modrinth",
                    ModSource::Curseforge => "curseforge",
                    ModSource::Ftb => "ftb", // FTB: pack-managed, not individually distributable.
                    ModSource::Atlauncher => "atlauncher", // ATLauncher: pack-managed, not individually distributable.
                }
                .into(),
                project_id: v.project_id.clone(),
            });
        }
        let sha = v
            .primary_file
            .sha1
            .as_deref()
            .ok_or(Error::ModsSha1Unavailable)?
            .to_ascii_lowercase();
        fetch_to_cache(
            data_dir,
            &v.primary_file.url,
            &sha,
            v.primary_file.size,
            "mods",
            progress,
        )
        .await?;
    }

    // Phase 2 — swap. Remove the old jar, then install from the warm
    // cache (install_one's internal fetch_to_cache is now a cache hit).
    uninstall(instance_root, old_sha1).await?;
    let primary = install_one(data_dir, instance_root, target, progress).await?;
    let mut deps = Vec::new();
    for d in required_deps {
        deps.push(install_one(data_dir, instance_root, d, progress).await?);
    }

    // install_one always lands a mod enabled — restore a disabled state.
    if !was_enabled {
        disable(instance_root, &primary.sha1).await?;
    }

    Ok(UpdateOutcome {
        primary,
        deps,
        removed_sha1: old_sha1.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

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

    #[test]
    fn content_kind_maps_asset_paths_only() {
        use crate::mods::platform::ContentKind;
        assert_eq!(
            content_kind_for_install_path("resourcepacks/Faithful.zip"),
            Some(ContentKind::ResourcePack)
        );
        assert_eq!(
            content_kind_for_install_path("shaderpacks/BSL.zip"),
            Some(ContentKind::Shader)
        );
        assert_eq!(content_kind_for_install_path("mods/sodium.jar"), None);
        assert_eq!(content_kind_for_install_path("config/sodium.toml"), None);
        assert_eq!(content_kind_for_install_path("options.txt"), None);
    }

    #[tokio::test]
    async fn install_one_rejects_unsafe_filename_before_io() {
        let _g = test_lock();
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        // Escaping filename from a hostile API response. No mock server / allowed
        // hosts are configured: the guard must reject before any network or
        // filesystem work happens.
        let v = fake_version(
            "http://127.0.0.1:1/evil.jar".into(),
            "0".repeat(40),
            3,
            "../../evil.jar",
        );
        let err = install_one(td_data.path(), td_inst.path(), v, &nop_progress())
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::ModsUnsafeFilename { ref filename } if filename == "../../evil.jar"),
            "expected ModsUnsafeFilename, got {err:?}"
        );
        // Nothing was written: neither an escaped file nor the mods dir itself.
        assert!(!td_inst.path().join("evil.jar").exists());
        assert!(!installed::mods_dir(td_inst.path()).exists());
    }

    #[tokio::test]
    async fn cold_download_populates_cache_and_installs() {
        let _g = test_lock();
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let installed = install_one(td_data.path(), td_inst.path(), v, &nop_progress())
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(installed.sha1, sha);
        assert!(installed::mods_dir(td_inst.path()).join("x.jar").exists());
        assert!(cache::cache_path_for(td_data.path(), &sha).exists());
        let list = installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source, Some(ModSource::Modrinth));
    }

    #[tokio::test]
    async fn idempotent_reinstall_succeeds() {
        let _g = test_lock();
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_one(td_data.path(), td_inst.path(), v(), &nop_progress())
            .await
            .unwrap();
        install_one(td_data.path(), td_inst.path(), v(), &nop_progress())
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn filename_conflict_with_different_sha_errors() {
        let _g = test_lock();
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = install_one(td_data.path(), td_inst.path(), v, &nop_progress())
            .await
            .unwrap_err();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "expected ModsFilenameConflict, got {err:?}"
        );
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
        let _g = test_lock();
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
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
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn install_asset_empty_sha1_rejects_before_io() {
        // An empty sha1 must be rejected immediately — no network I/O, no
        // directory creation — so the no-TOFU guard fires before any side-effect.
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        // Pass a dummy URL that would trigger a real request if not guarded.
        let r = install_asset(
            td_data.path(),
            td_inst.path(),
            "http://127.0.0.1:1/unreachable.zip",
            "",
            100.0,
            "resourcepacks/RP.zip",
            &nop_progress(),
        )
        .await;
        assert!(
            matches!(r, Err(Error::ModsSha1Unavailable)),
            "empty sha1 must return ModsSha1Unavailable, got {r:?}"
        );
        // Guard must also fire for whitespace-only sha1.
        let r2 = install_asset(
            td_data.path(),
            td_inst.path(),
            "http://127.0.0.1:1/unreachable.zip",
            "   ",
            100.0,
            "resourcepacks/RP.zip",
            &nop_progress(),
        )
        .await;
        assert!(
            matches!(r2, Err(Error::ModsSha1Unavailable)),
            "whitespace sha1 must return ModsSha1Unavailable, got {r2:?}"
        );
    }

    #[tokio::test]
    async fn fetch_to_cache_empty_sha1_rejects_before_io() {
        // No-TOFU: an empty sha1 must be rejected immediately — before any network
        // I/O or cache write — so the mod sink is fail-closed, not just the asset sink.
        let data = TempDir::new().unwrap();
        let noop: ProgressFn = Box::new(|_, _, _| {});
        let r = fetch_to_cache(
            data.path(),
            "https://edge.forgecdn.net/files/1/2/x.jar",
            "", // empty sha — must reject
            100.0,
            "mods",
            &noop,
        )
        .await;
        assert!(
            matches!(r, Err(Error::ModsSha1Unavailable)),
            "empty sha must be rejected with ModsSha1Unavailable, got {r:?}"
        );
        // Guard must also fire for whitespace-only sha1.
        let r2 = fetch_to_cache(
            data.path(),
            "https://edge.forgecdn.net/files/1/2/x.jar",
            "   ", // whitespace-only sha — must also reject
            100.0,
            "mods",
            &noop,
        )
        .await;
        assert!(
            matches!(r2, Err(Error::ModsSha1Unavailable)),
            "whitespace sha must be rejected with ModsSha1Unavailable, got {r2:?}"
        );
    }

    #[tokio::test]
    async fn install_asset_writes_to_declared_path() {
        let _g = test_lock();
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
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
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
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
        let _g = test_lock();
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_one(td_data.path(), td_inst.path(), v, &nop_progress())
            .await
            .unwrap();
        uninstall(td_inst.path(), &sha).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(!installed::mods_dir(td_inst.path()).join("u.jar").exists());
        assert!(cache::cache_path_for(td_data.path(), &sha).exists()); // cache survives
        assert!(installed::list(td_inst.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_one_swaps_the_installed_version() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let v1_bytes = b"version-one";
        let v2_bytes = b"version-two";
        let v1_sha = hex::encode(Sha1::digest(v1_bytes));
        let v2_sha = hex::encode(Sha1::digest(v2_bytes));
        Mock::given(method("GET"))
            .and(path("/v1.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(v1_bytes.to_vec()))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(v2_bytes.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let v1 = fake_version(
            format!("{}/v1.jar", s.uri()),
            v1_sha.clone(),
            v1_bytes.len() as u64,
            "v1.jar",
        );
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_one(td_data.path(), td_inst.path(), v1, &nop_progress())
            .await
            .unwrap();
        let v2 = fake_version(
            format!("{}/v2.jar", s.uri()),
            v2_sha.clone(),
            v2_bytes.len() as u64,
            "v2.jar",
        );
        let outcome = update_one(
            td_data.path(),
            td_inst.path(),
            &v1_sha,
            v2,
            vec![],
            &nop_progress(),
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(outcome.removed_sha1, v1_sha);
        assert_eq!(outcome.primary.sha1, v2_sha);
        assert!(installed::mods_dir(td_inst.path()).join("v2.jar").exists());
        assert!(!installed::mods_dir(td_inst.path()).join("v1.jar").exists());
        let list = installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].sha1, v2_sha);
    }

    #[tokio::test]
    async fn update_one_preserves_disabled_state() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let v1b = b"d-one";
        let v2b = b"d-two";
        let v1s = hex::encode(Sha1::digest(v1b));
        let v2s = hex::encode(Sha1::digest(v2b));
        Mock::given(method("GET"))
            .and(path("/d1.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(v1b.to_vec()))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/d2.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(v2b.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_one(
            td_data.path(),
            td_inst.path(),
            fake_version(
                format!("{}/d1.jar", s.uri()),
                v1s.clone(),
                v1b.len() as u64,
                "d1.jar",
            ),
            &nop_progress(),
        )
        .await
        .unwrap();
        disable(td_inst.path(), &v1s).await.unwrap();
        let v2 = fake_version(
            format!("{}/d2.jar", s.uri()),
            v2s.clone(),
            v2b.len() as u64,
            "d2.jar",
        );
        update_one(
            td_data.path(),
            td_inst.path(),
            &v1s,
            v2,
            vec![],
            &nop_progress(),
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(installed::mods_dir(td_inst.path())
            .join("d2.jar.disabled")
            .exists());
        let list = installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled);
    }

    #[tokio::test]
    async fn update_one_aborts_before_swap_when_download_fails() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let v1b = b"keep-me";
        let v1s = hex::encode(Sha1::digest(v1b));
        Mock::given(method("GET"))
            .and(path("/k1.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(v1b.to_vec()))
            .mount(&s)
            .await;
        // No mock for /missing.jar — wiremock answers 404, so the
        // pre-warm download fails before the swap.
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_one(
            td_data.path(),
            td_inst.path(),
            fake_version(
                format!("{}/k1.jar", s.uri()),
                v1s.clone(),
                v1b.len() as u64,
                "k1.jar",
            ),
            &nop_progress(),
        )
        .await
        .unwrap();
        let bad = fake_version(
            format!("{}/missing.jar", s.uri()),
            "ffff".into(),
            5,
            "missing.jar",
        );
        let r = update_one(
            td_data.path(),
            td_inst.path(),
            &v1s,
            bad,
            vec![],
            &nop_progress(),
        )
        .await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(r.is_err());
        // The old version must be untouched.
        assert!(installed::mods_dir(td_inst.path()).join("k1.jar").exists());
        let list = installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].sha1, v1s);
    }

    #[tokio::test]
    async fn update_one_installs_required_deps() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let oldb = b"primary-v1";
        let pb = b"primary-v2";
        let db = b"dep-bytes";
        let olds = hex::encode(Sha1::digest(oldb));
        let ps = hex::encode(Sha1::digest(pb));
        let ds = hex::encode(Sha1::digest(db));
        for (p, body) in [
            ("/old.jar", oldb.to_vec()),
            ("/p2.jar", pb.to_vec()),
            ("/dep.jar", db.to_vec()),
        ] {
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .mount(&s)
                .await;
        }
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_one(
            td_data.path(),
            td_inst.path(),
            fake_version(
                format!("{}/old.jar", s.uri()),
                olds.clone(),
                oldb.len() as u64,
                "old.jar",
            ),
            &nop_progress(),
        )
        .await
        .unwrap();
        let target = fake_version(
            format!("{}/p2.jar", s.uri()),
            ps.clone(),
            pb.len() as u64,
            "p2.jar",
        );
        let dep = fake_version(
            format!("{}/dep.jar", s.uri()),
            ds.clone(),
            db.len() as u64,
            "dep.jar",
        );
        update_one(
            td_data.path(),
            td_inst.path(),
            &olds,
            target,
            vec![dep],
            &nop_progress(),
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(installed::mods_dir(td_inst.path()).join("p2.jar").exists());
        assert!(installed::mods_dir(td_inst.path()).join("dep.jar").exists());
        let list = installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn update_one_rejects_distribution_disabled_target() {
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let mut v = fake_version("https://example/x.jar".into(), "aa".into(), 0, "x.jar");
        v.primary_file.distribution_allowed = false;
        let r = update_one(
            td_data.path(),
            td_inst.path(),
            "nonexistent",
            v,
            vec![],
            &nop_progress(),
        )
        .await;
        assert!(matches!(r, Err(Error::ModsDistributionDisabled { .. })));
    }

    #[tokio::test]
    async fn update_one_rejects_unsafe_target_filename_before_io() {
        let _g = test_lock();
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        // Hostile filename on the update target. No mock server / allowed hosts:
        // the Phase-1 guard must reject before any cache-warming network I/O.
        let v = fake_version(
            "http://127.0.0.1:1/evil.jar".into(),
            "0".repeat(40),
            0,
            "../../evil.jar",
        );
        let r = update_one(
            td_data.path(),
            td_inst.path(),
            "nonexistent",
            v,
            vec![],
            &nop_progress(),
        )
        .await;
        assert!(
            matches!(&r, Err(Error::ModsUnsafeFilename { filename }) if filename == "../../evil.jar"),
            "expected ModsUnsafeFilename, got {r:?}"
        );
        assert!(!installed::mods_dir(td_inst.path()).exists());
    }

    #[tokio::test]
    async fn install_asset_tracked_routes_shader_and_records() {
        use crate::mods::platform::{ContentKind, ModSource};
        let _g = test_lock();
        let body = b"shader-bytes";
        let sha = hex::encode(Sha1::digest(body));
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/Complementary-r5.3.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_asset_tracked(
            td_data.path(),
            td_inst.path(),
            ContentKind::Shader,
            Some(ModSource::Modrinth),
            Some("proj".into()),
            Some("ver".into()),
            "Complementary",
            Some("r5.3".into()),
            "Complementary-r5.3.zip",
            &format!("{}/Complementary-r5.3.zip", s.uri()),
            Some(&sha),
            body.len() as f64,
            &nop_progress(),
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(td_inst
            .path()
            .join(".minecraft/shaderpacks/Complementary-r5.3.zip")
            .exists());
        let listed = crate::mods::assets::list(td_inst.path(), ContentKind::Shader)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "Complementary-r5.3.zip");
        assert_eq!(listed[0].sha1, sha.to_ascii_lowercase());
    }

    #[tokio::test]
    async fn install_asset_tracked_routes_resourcepack() {
        use crate::mods::platform::{ContentKind, ModSource};
        let _g = test_lock();
        let body = b"resourcepack-tracked-bytes";
        let sha = hex::encode(Sha1::digest(body));
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/Faithful.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        install_asset_tracked(
            td_data.path(),
            td_inst.path(),
            ContentKind::ResourcePack,
            Some(ModSource::Modrinth),
            Some("fp".into()),
            Some("fv".into()),
            "Faithful",
            Some("1.20".into()),
            "Faithful.zip",
            &format!("{}/Faithful.zip", s.uri()),
            Some(&sha),
            body.len() as f64,
            &nop_progress(),
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(td_inst
            .path()
            .join(".minecraft/resourcepacks/Faithful.zip")
            .exists());
        let listed = crate::mods::assets::list(td_inst.path(), ContentKind::ResourcePack)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "Faithful.zip");
    }

    #[tokio::test]
    async fn install_asset_tracked_none_sha_rejects_no_file_no_registry() {
        // No-TOFU: a missing SHA-1 must abort before any IO — no file on disk
        // and no registry entry. Mirrors install_one's ModsSha1Unavailable gate.
        use crate::mods::platform::ContentKind;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let r = install_asset_tracked(
            td_data.path(),
            td_inst.path(),
            ContentKind::Shader,
            None,
            None,
            None,
            "NoSha",
            None,
            "NoSha.zip",
            "http://127.0.0.1:1/unreachable.zip",
            None,
            100.0,
            &nop_progress(),
        )
        .await;
        assert!(
            matches!(r, Err(Error::ModsSha1Unavailable)),
            "None sha must return ModsSha1Unavailable, got {r:?}"
        );
        // No file written.
        assert!(!td_inst
            .path()
            .join(".minecraft/shaderpacks/NoSha.zip")
            .exists());
        // No registry entry.
        let listed = crate::mods::assets::list(td_inst.path(), ContentKind::Shader)
            .await
            .unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn fetch_to_cache_md5_warms_cache_and_returns_sha1() {
        use md5::Digest as _;
        let _g = test_lock();
        let body = b"atl-md5-mod";
        let md5_hex = hex::encode(md5::Md5::digest(body));
        let sha1_hex = hex::encode(Sha1::digest(body));
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/m.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let data = TempDir::new().unwrap();
        let noop: ProgressFn = Box::new(|_, _, _| {});
        let (path, got_sha1) = fetch_to_cache_md5(
            data.path(),
            &format!("{}/m.jar", s.uri()),
            &md5_hex,
            body.len() as f64,
            "modpacks",
            &noop,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(got_sha1, sha1_hex);
        assert!(
            path.exists(),
            "cache entry must exist under the computed sha1"
        );
        assert_eq!(path, cache::cache_path_for(data.path(), &sha1_hex));
    }

    #[test]
    fn safe_asset_remove_path_rejects_escape() {
        use crate::mods::platform::ContentKind;
        let root = std::path::Path::new("/tmp/inst");
        // Traversal with ..
        assert!(
            safe_asset_remove_path(root, ContentKind::Shader, "../../evil").is_err(),
            "../../evil should be rejected"
        );
        // Backslash separator
        assert!(
            safe_asset_remove_path(root, ContentKind::Shader, r"sub\evil.zip").is_err(),
            r"sub\evil.zip should be rejected"
        );
        // Absolute path component
        assert!(
            safe_asset_remove_path(root, ContentKind::Shader, "/abs/evil.zip").is_err(),
            "/abs/evil.zip should be rejected"
        );
        // Empty filename produces empty rel → rejected
        assert!(
            safe_asset_remove_path(root, ContentKind::ResourcePack, "").is_err(),
            "empty filename should be rejected"
        );
        // Normal basename is accepted
        assert!(
            safe_asset_remove_path(root, ContentKind::Shader, "Complementary.zip").is_ok(),
            "Complementary.zip should be accepted"
        );
        assert!(
            safe_asset_remove_path(root, ContentKind::ResourcePack, "Faithful.zip").is_ok(),
            "Faithful.zip should be accepted"
        );
    }
}
