//! Loader matrix e2e: install + launch every (MC, loader) combination,
//! wait for menu marker in MC's `latest.log`, kill the process, move on.
//!
//! # Why this exists
//!
//! Manual gating one combo at a time misses systemic regressions across
//! the loader/version cross-product (e.g. URL-less library filter that
//! broke modern Forge launch but not transitional). This harness drives
//! the whole matrix in one shot so failure modes are catalogued upfront.
//!
//! # Running
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test loader_matrix_e2e \
//!     -- --ignored --nocapture
//! ```
//!
//! Expect 1.5-3 hours of wall clock and ~5-10 GB of disk + network.
//! Each combo writes a per-combo log to `target/loader-matrix-logs/`.
//! A summary table is printed at the end.
//!
//! # AppHandle strategy
//!
//! Same as `forge_modern_era_e2e.rs` — replicate the install pipeline
//! using public helpers and a tempdir for the data root. We never
//! construct a Wry AppHandle (would need WebView2 DLLs in test PATH).

use lucerna_lib::forge::installer::{detect_era, Era};
use lucerna_lib::forge::patcher::{maven_coord_to_relative_path, run_processor, ProcessorContext};
use lucerna_lib::network;
use lucerna_lib::versions::libraries::artifacts_to_install;
use lucerna_lib::versions::loaders::{list_loaders, synth_id, Loader};
use lucerna_lib::versions::manifest::list_manifest;
use lucerna_lib::versions::resolve::merge_inherits;
use lucerna_lib::versions::version_json::{parse as parse_version_json, Library, VersionDetails};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Matrix-local loader enum that includes vanilla. `versions::loaders::Loader`
/// only enumerates real loaders (Fabric/Quilt/Forge); the vanilla case is the
/// absence of a loader and gets its own variant here for clean dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixLoader {
    Vanilla,
    Real(Loader),
}

impl MatrixLoader {
    fn as_label(self) -> &'static str {
        match self {
            MatrixLoader::Vanilla => "vanilla",
            MatrixLoader::Real(Loader::Fabric) => "fabric",
            MatrixLoader::Real(Loader::Quilt) => "quilt",
            MatrixLoader::Real(Loader::Forge) => "forge",
            MatrixLoader::Real(Loader::NeoForge) => "neoforge",
        }
    }
}

// ─── Matrix definition ──────────────────────────────────────────────────────

/// MC version coverage. Picked to span every Forge/Fabric/Quilt era.
///
/// Override via LUCERNA_MATRIX_MC=mc1,mc2,... to run a subset (useful
/// during harness debugging — full matrix is 1.5-3h, single MC is ~10 min).
const MC_VERSIONS_DEFAULT: &[&str] = &[
    "1.7.10",  // legacy Forge boundary, no Fabric/Quilt
    "1.12.2",  // popular modpack target, still legacy Forge
    "1.16.5",  // transitional Forge era, early Fabric/Quilt
    "1.18.2",  // first MC requiring Java 17, modern Forge era
    "1.19.4",  // pre-1.20 stable
    "1.20.4",  // current "modern stable" used by Phase 3 e2e
    "1.21", // Mojang publishes the .0 release without the patch suffix (manifest id is "1.21", not "1.21.0")
    "1.21.1", // NeoForge 21.1.x boundary (FML 4.0.42, no merge); caught by manual gate 2026-05-17
    "1.21.8", // installertools:3.0.13 MERGE_MAPPING arg rename (--merge/--base) boundary
    "1.21.10", // 1.21.x stable
    "1.21.11", // very latest at audit time
];

fn mc_versions() -> Vec<String> {
    match std::env::var("LUCERNA_MATRIX_MC") {
        Ok(s) if !s.trim().is_empty() => s.split(',').map(|s| s.trim().to_string()).collect(),
        _ => MC_VERSIONS_DEFAULT.iter().map(|s| s.to_string()).collect(),
    }
}

/// NeoForge applies to MC >= 1.20.1. Returns `false` for 1.20.0 (NeoForge
/// fork happened at 1.20.1) and any MC below 1.20.
fn neoforge_applies(mc: &str) -> bool {
    let parts: Vec<u32> = mc.split('.').filter_map(|s| s.parse().ok()).collect();
    let major = parts.first().copied().unwrap_or(0);
    let minor = parts.get(1).copied().unwrap_or(0);
    let patch = parts.get(2).copied().unwrap_or(0);
    if major > 1 {
        return true;
    }
    if major < 1 {
        return false;
    }
    if minor > 20 {
        return true;
    }
    if minor < 20 {
        return false;
    }
    patch >= 1
}

/// Per-MC loader applicability.
fn loaders_for(mc: &str) -> Vec<MatrixLoader> {
    let mut out = vec![MatrixLoader::Vanilla];
    let parts: Vec<u32> = mc.split('.').filter_map(|s| s.parse().ok()).collect();
    let mm = (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
    );
    if mm >= (1, 16) {
        out.push(MatrixLoader::Real(Loader::Fabric));
        out.push(MatrixLoader::Real(Loader::Quilt));
    }
    if mm >= (1, 6) {
        out.push(MatrixLoader::Real(Loader::Forge));
    }
    if neoforge_applies(mc) {
        out.push(MatrixLoader::Real(Loader::NeoForge));
    }
    out
}

#[cfg(test)]
mod loaders_for_tests {
    use super::*;
    use lucerna_lib::versions::loaders::Loader;

    #[test]
    fn neoforge_excluded_for_1_20_0() {
        assert!(!loaders_for("1.20.0")
            .iter()
            .any(|l| matches!(l, MatrixLoader::Real(Loader::NeoForge))));
    }

    #[test]
    fn neoforge_included_for_1_20_1() {
        assert!(loaders_for("1.20.1")
            .iter()
            .any(|l| matches!(l, MatrixLoader::Real(Loader::NeoForge))));
    }

    #[test]
    fn neoforge_included_for_1_21_x() {
        assert!(loaders_for("1.21.10")
            .iter()
            .any(|l| matches!(l, MatrixLoader::Real(Loader::NeoForge))));
    }

    #[test]
    fn neoforge_excluded_for_1_16_5() {
        assert!(!loaders_for("1.16.5")
            .iter()
            .any(|l| matches!(l, MatrixLoader::Real(Loader::NeoForge))));
    }
}

// ─── Combo + Outcome types ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Combo {
    mc: String,
    loader: MatrixLoader,
    /// Empty for vanilla; recommended loader version otherwise.
    #[allow(dead_code)]
    loader_version: String,
}

impl Combo {
    fn label(&self) -> String {
        match self.loader {
            MatrixLoader::Vanilla => format!("{}/vanilla", self.mc),
            MatrixLoader::Real(l) => format!("{}/{}-{}", self.mc, l.as_str(), self.loader_version),
        }
    }
    fn log_filename(&self) -> String {
        self.label().replace('/', "_").replace(':', "_")
    }
    fn synth_id(&self) -> String {
        match self.loader {
            MatrixLoader::Vanilla => self.mc.clone(),
            MatrixLoader::Real(l) => synth_id(l, &self.loader_version, &self.mc),
        }
    }
}

#[derive(Debug)]
enum Outcome {
    Success {
        menu_at: Duration,
        install_at: Duration,
    },
    InstallFailed {
        stage: String,
        error: String,
    },
    LaunchSpawnFailed {
        error: String,
    },
    LaunchCrashed {
        exit_code: Option<i32>,
        log_tail: String,
    },
    LaunchTimedOut {
        log_tail: String,
    },
}

impl Outcome {
    fn summary(&self) -> String {
        match self {
            Outcome::Success {
                menu_at,
                install_at,
            } => {
                format!("OK (install {:?}, menu {:?})", install_at, menu_at)
            }
            Outcome::InstallFailed { stage, error } => {
                let snippet: String = error.chars().take(160).collect();
                format!("INSTALL_FAILED@{stage}: {snippet}")
            }
            Outcome::LaunchSpawnFailed { error } => format!("SPAWN_FAILED: {error}"),
            Outcome::LaunchCrashed { exit_code, .. } => {
                format!("LAUNCH_CRASHED exit={exit_code:?}")
            }
            Outcome::LaunchTimedOut { .. } => "LAUNCH_TIMED_OUT (no menu marker in 120s)".into(),
        }
    }
    fn is_success(&self) -> bool {
        matches!(self, Outcome::Success { .. })
    }
}

struct InstallError {
    stage: String,
    error: String,
}

// ─── Path layout helpers ────────────────────────────────────────────────────

struct Paths {
    root: PathBuf,
}
impl Paths {
    fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }
    fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    fn forge_cache(&self, mc: &str, fv: &str) -> PathBuf {
        self.root
            .join("forge")
            .join("cache")
            .join(format!("{mc}-{fv}"))
    }
    fn installer_jar(&self, mc: &str, fv: &str) -> PathBuf {
        self.root
            .join("forge")
            .join("installers")
            .join(format!("{mc}-{fv}.jar"))
    }
    fn installer_jar_neoforge(&self, nv: &str) -> PathBuf {
        self.root
            .join("forge")
            .join("installers")
            .join(format!("neoforge-{nv}.jar"))
    }
    fn game_dir(&self, version_id: &str) -> PathBuf {
        self.root.join("game").join(version_id)
    }
    fn natives_dir(&self, version_id: &str) -> PathBuf {
        self.root.join("natives").join(version_id)
    }
    fn vanilla_client_jar(&self, mc: &str) -> PathBuf {
        self.versions().join(mc).join(format!("{mc}.jar"))
    }
}

// ─── Network helpers ────────────────────────────────────────────────────────

async fn download_file(url: &str, dest: &Path, sha1: &str) -> Result<(), InstallError> {
    // Cache hit?
    if sha1.is_empty() {
        if tokio::fs::metadata(dest).await.is_ok() {
            return Ok(());
        }
    } else if let Ok(bytes) = tokio::fs::read(dest).await {
        use sha1::{Digest, Sha1};
        let got = hex::encode(Sha1::digest(&bytes));
        if got == sha1 {
            return Ok(());
        }
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| InstallError {
                stage: "mkdir".into(),
                error: format!("{}: {e}", parent.display()),
            })?;
    }
    network::download::download_no_emit(url, dest, sha1, "matrix")
        .await
        .map_err(|e| InstallError {
            stage: "download".into(),
            error: format!("{url}: {e:?}"),
        })
}

// ─── Vanilla install ────────────────────────────────────────────────────────

async fn fetch_vanilla_details(mc: &str) -> Result<VersionDetails, InstallError> {
    let manifest = list_manifest().await.map_err(|e| InstallError {
        stage: "manifest".into(),
        error: format!("{e:?}"),
    })?;
    let entry = manifest
        .iter()
        .find(|e| e.id == mc)
        .ok_or_else(|| InstallError {
            stage: "manifest".into(),
            error: format!("{mc} not in Mojang manifest"),
        })?;
    let v: serde_json::Value = network::get_json(&entry.url, "matrix-version")
        .await
        .map_err(|e| InstallError {
            stage: "version-json".into(),
            error: format!("{e:?}"),
        })?;
    let text = serde_json::to_string(&v).map_err(|e| InstallError {
        stage: "version-json".into(),
        error: format!("re-serialise: {e}"),
    })?;
    parse_version_json(&text).map_err(|e| InstallError {
        stage: "version-json".into(),
        error: format!("parse: {e}"),
    })
}

async fn install_vanilla(mc: &str, paths: &Paths) -> Result<VersionDetails, InstallError> {
    let details = fetch_vanilla_details(mc).await?;
    download_vanilla_payload(&details, mc, paths).await?;
    Ok(details)
}

async fn download_vanilla_payload(
    details: &VersionDetails,
    version_id: &str,
    paths: &Paths,
) -> Result<(), InstallError> {
    let os = detect_os();
    let arch = detect_arch();

    // Libraries.
    for lib in &details.libraries {
        for (rel, url, sha1, _size) in artifacts_to_install(lib, os, arch) {
            if url.is_empty() {
                // Locally-produced (Forge {PATCHED}); already on disk after
                // forge install. Skip verification at vanilla layer.
                continue;
            }
            download_file(&url, &paths.libraries().join(&rel), &sha1).await?;
        }
    }

    // Client jar.
    if let Some(dl) = &details.downloads {
        let dest = paths
            .versions()
            .join(version_id)
            .join(format!("{version_id}.jar"));
        download_file(&dl.client.url, &dest, &dl.client.sha1).await?;
    }

    // Asset index + objects.
    if let Some(idx) = &details.asset_index {
        let idx_url = &idx.url;
        let idx_path = paths
            .assets()
            .join("indexes")
            .join(format!("{}.json", idx.id));
        download_file(idx_url, &idx_path, &idx.sha1).await?;
        let idx_value: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(&idx_path).await.map_err(|e| {
                InstallError {
                    stage: "assets".into(),
                    error: format!("read {}: {e}", idx_path.display()),
                }
            })?)
            .map_err(|e| InstallError {
                stage: "assets".into(),
                error: format!("parse asset index: {e}"),
            })?;
        if let Some(objects) = idx_value.get("objects").and_then(|v| v.as_object()) {
            for (_name, obj) in objects {
                let hash = obj.get("hash").and_then(|v| v.as_str()).unwrap_or("");
                if hash.len() < 2 {
                    continue;
                }
                let prefix = &hash[..2];
                let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
                let dest = paths.assets().join("objects").join(prefix).join(hash);
                download_file(&url, &dest, hash).await?;
            }
        }
    }
    Ok(())
}

// ─── Fabric / Quilt install ─────────────────────────────────────────────────

async fn install_fabric_or_quilt(
    combo: &Combo,
    paths: &Paths,
) -> Result<VersionDetails, InstallError> {
    // 1. Vanilla first (provides client jar + base libraries).
    let vanilla = install_vanilla(&combo.mc, paths).await?;
    // 2. Loader profile (the `fetch_profile` call doesn't need AppHandle for fabric/quilt).
    // We need to pass an AppHandle... wait, no — fetch_profile dispatches per loader. For
    // fabric/quilt it just hits the meta endpoint via `network::get_json`. The `app` arg is
    // only used by Forge's branch. We can pass a dummy via mocking.
    //
    // Workaround: call the underlying module-level `profile()` functions directly by hand.
    // They live in `loaders::{fabric, quilt}::profile(mc, ver)` (pub(super)). Since they're
    // pub(super) we can't reach them from tests. Instead, replicate the fetch.
    let loader_profile = fetch_loader_profile_directly(combo).await?;
    // 3. Merge.
    let merged = merge_loader_into_vanilla(&combo.synth_id(), &loader_profile, vanilla);
    // 4. Download loader-specific libs (the new entries vs vanilla).
    download_vanilla_payload(&merged, &combo.synth_id(), paths).await?;
    Ok(merged)
}

/// Bypass `fetch_profile` (which expects AppHandle for Forge). For Fabric/Quilt
/// hit the meta endpoint directly. URL format same as in `loaders::{fabric, quilt}`.
async fn fetch_loader_profile_directly(combo: &Combo) -> Result<VersionDetails, InstallError> {
    let inner = match combo.loader {
        MatrixLoader::Real(l) => l,
        MatrixLoader::Vanilla => unreachable!("vanilla handled separately"),
    };
    let base = match inner {
        Loader::Fabric => "https://meta.fabricmc.net/v2",
        Loader::Quilt => "https://meta.quiltmc.org/v3",
        Loader::Forge | Loader::NeoForge => unreachable!("forge/neoforge handled separately"),
    };
    let url = format!(
        "{base}/versions/loader/{}/{}/profile/json",
        combo.mc, combo.loader_version
    );
    let stage = format!("{}-profile", combo.loader.as_label());
    let raw: serde_json::Value = network::get_json(&url, "matrix-loader-profile")
        .await
        .map_err(|e| InstallError {
            stage: stage.clone(),
            error: format!("{e:?}"),
        })?;
    let text = serde_json::to_string(&raw).map_err(|e| InstallError {
        stage: stage.clone(),
        error: format!("re-serialise: {e}"),
    })?;
    let mut details = parse_version_json(&text).map_err(|e| InstallError {
        stage,
        error: format!("parse: {e}"),
    })?;

    // For Quilt 0.20+: inject `hashed`+`intermediary` mappings from the meta-list entry
    // (mirrors `loaders::quilt::profile`'s post-processing).
    if inner == Loader::Quilt {
        let list_url = format!("{base}/versions/loader/{}", combo.mc);
        let list_raw: serde_json::Value = network::get_json(&list_url, "matrix-loader-list")
            .await
            .map_err(|e| InstallError {
                stage: "quilt-list".into(),
                error: format!("{e:?}"),
            })?;
        if let Some(entries) = list_raw.as_array() {
            if let Some(entry) = entries.iter().find(|e| {
                e.get("loader")
                    .and_then(|l| l.get("version"))
                    .and_then(|v| v.as_str())
                    == Some(combo.loader_version.as_str())
            }) {
                for key in &["hashed", "intermediary"] {
                    if let Some(maven) = entry
                        .get(*key)
                        .and_then(|m| m.get("maven"))
                        .and_then(|v| v.as_str())
                    {
                        let already = details.libraries.iter().any(|l| {
                            let lkey = l.name.split(':').take(2).collect::<Vec<_>>().join(":");
                            let mkey = maven.split(':').take(2).collect::<Vec<_>>().join(":");
                            lkey == mkey
                        });
                        if !already {
                            let url = if maven.starts_with("net.fabricmc:") {
                                "https://maven.fabricmc.net/"
                            } else {
                                "https://maven.quiltmc.org/repository/release/"
                            };
                            details.libraries.push(Library {
                                name: maven.to_string(),
                                url: Some(url.into()),
                                downloads: None,
                                rules: None,
                                natives: None,
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(details)
}

/// Thin wrapper that fixes up the child's `inheritsFrom`/`id` so
/// `merge_inherits` produces the right synth-id'd merged details.
fn merge_loader_into_vanilla(
    synth_id: &str,
    loader_profile: &VersionDetails,
    vanilla: VersionDetails,
) -> VersionDetails {
    let mut child = loader_profile.clone();
    child.id = synth_id.to_string();
    child.inherits_from = Some(vanilla.id.clone());
    merge_inherits(child, vanilla)
}

// ─── Forge install ──────────────────────────────────────────────────────────

async fn install_forge(combo: &Combo, paths: &Paths) -> Result<VersionDetails, InstallError> {
    // 1. Vanilla parent (Forge inherits from it).
    let vanilla = install_vanilla(&combo.mc, paths).await?;
    let _ = vanilla; // used implicitly via downloaded artifacts

    // 2. Download Forge installer.
    // Some legacy MC versions ship with the `<mc>-<fv>-<mc>` maven path quirk
    // (1.7.10 most notably). Mirrors the allowlist in `fetch.ps1`.
    let quirk_mcs = ["1.7.10"];
    let installer_path = paths.installer_jar(&combo.mc, &combo.loader_version);
    if tokio::fs::metadata(&installer_path).await.is_err() {
        let seg_normal = forge_path_segment(&combo.mc, &combo.loader_version);
        let (segment, filename) = if quirk_mcs.contains(&combo.mc.as_str()) {
            let q_seg = format!("{seg_normal}-{}", combo.mc);
            let q_name = format!("forge-{q_seg}-installer.jar");
            (q_seg, q_name)
        } else {
            (
                seg_normal.clone(),
                format!("forge-{seg_normal}-installer.jar"),
            )
        };
        let url = format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{segment}/{filename}"
        );
        download_file(&url, &installer_path, "").await?;
    }
    let installer_bytes = tokio::fs::read(&installer_path)
        .await
        .map_err(|e| InstallError {
            stage: "forge-installer-read".into(),
            error: format!("{}: {e}", installer_path.display()),
        })?;

    // 3. Parse install_profile + dispatch by era.
    let profile_value = read_install_profile(&installer_bytes)?;
    let era = detect_era(&profile_value);
    match era {
        Era::Legacy => install_forge_legacy(combo, paths, &installer_bytes, &profile_value).await,
        Era::Transitional => {
            install_forge_processor_era(combo, paths, &installer_bytes, &profile_value, false).await
        }
        Era::Modern => {
            install_forge_processor_era(combo, paths, &installer_bytes, &profile_value, true).await
        }
    }
}

async fn install_neoforge(combo: &Combo, paths: &Paths) -> Result<VersionDetails, InstallError> {
    // 1. Vanilla parent (NeoForge inherits from it, same as Forge).
    let vanilla = install_vanilla(&combo.mc, paths).await?;
    let _ = vanilla;

    // 2. Download NeoForge installer.
    // NeoForge layout: maven.neoforged.net/releases/.../neoforge/<nv>/neoforge-<nv>-installer.jar.
    // No <mc>-<fv>-<mc> quirk (only ever modern-era MC versions).
    let nv = &combo.loader_version;
    let installer_path = paths.installer_jar_neoforge(nv);
    if tokio::fs::metadata(&installer_path).await.is_err() {
        let url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{nv}/neoforge-{nv}-installer.jar"
        );
        download_file(&url, &installer_path, "").await?;
    }
    let installer_bytes = tokio::fs::read(&installer_path)
        .await
        .map_err(|e| InstallError {
            stage: "neoforge-installer-read".into(),
            error: format!("{}: {e}", installer_path.display()),
        })?;

    // 3. Parse install_profile + dispatch by era.
    // NeoForge is always modern (spec=1).
    let profile_value = read_install_profile(&installer_bytes)?;
    let era = detect_era(&profile_value);
    if era != Era::Modern {
        return Err(InstallError {
            stage: "neoforge-era".into(),
            error: format!("NeoForge installer must be modern era; got {:?}", era),
        });
    }
    install_forge_processor_era(combo, paths, &installer_bytes, &profile_value, true).await
}

fn forge_path_segment(mc: &str, fv: &str) -> String {
    format!("{mc}-{fv}")
}

fn forge_installer_url(mc: &str, fv: &str) -> (String, String) {
    let seg = forge_path_segment(mc, fv);
    (
        format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{seg}/forge-{seg}-installer.jar"),
        format!("forge-{seg}-installer.jar"),
    )
}

fn read_install_profile(installer_bytes: &[u8]) -> Result<serde_json::Value, InstallError> {
    use std::io::Read;
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| InstallError {
            stage: "forge-installer-zip".into(),
            error: format!("{e}"),
        })?;
    let mut entry = zip
        .by_name("install_profile.json")
        .map_err(|e| InstallError {
            stage: "forge-install-profile".into(),
            error: format!("{e}"),
        })?;
    let mut s = String::new();
    entry.read_to_string(&mut s).map_err(|e| InstallError {
        stage: "forge-install-profile".into(),
        error: format!("read: {e}"),
    })?;
    serde_json::from_str(&s).map_err(|e| InstallError {
        stage: "forge-install-profile".into(),
        error: format!("parse: {e}"),
    })
}

async fn install_forge_legacy(
    combo: &Combo,
    paths: &Paths,
    installer_bytes: &[u8],
    profile_value: &serde_json::Value,
) -> Result<VersionDetails, InstallError> {
    use std::io::Read;
    // Step 1: Extract the universal jar from `install.filePath` inside the
    // installer ZIP, then place it at `install.path`'s maven-coord location.
    // The legacy library list references `forge-<v>.jar` (no `-universal`
    // classifier) — Forge's GUI installer renames it on the way out, so we do
    // the same. Without this, the library entry 404s at download.
    let file_path = profile_value
        .get("install")
        .and_then(|i| i.get("filePath"))
        .and_then(|p| p.as_str())
        .ok_or_else(|| InstallError {
            stage: "forge-legacy".into(),
            error: "missing install.filePath".into(),
        })?;
    let install_coord = profile_value
        .get("install")
        .and_then(|i| i.get("path"))
        .and_then(|p| p.as_str())
        .ok_or_else(|| InstallError {
            stage: "forge-legacy".into(),
            error: "missing install.path".into(),
        })?;
    let rel = maven_coord_to_relative_path(install_coord).ok_or_else(|| InstallError {
        stage: "forge-legacy".into(),
        error: format!("install.path not a maven coord: {install_coord}"),
    })?;
    let universal_bytes = {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| {
            InstallError {
                stage: "forge-legacy".into(),
                error: format!("zip: {e}"),
            }
        })?;
        let mut entry = zip.by_name(file_path).map_err(|e| InstallError {
            stage: "forge-legacy".into(),
            error: format!("entry {file_path}: {e}"),
        })?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| InstallError {
            stage: "forge-legacy".into(),
            error: format!("read {file_path}: {e}"),
        })?;
        buf
    };
    let dest = paths.libraries().join(&rel);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&dest, &universal_bytes)
        .await
        .map_err(|e| InstallError {
            stage: "forge-legacy".into(),
            error: format!("write {}: {e}", dest.display()),
        })?;

    // Step 2: Build merged VersionDetails (versionInfo merged with vanilla).
    let version_info = profile_value
        .get("versionInfo")
        .ok_or_else(|| InstallError {
            stage: "forge-legacy".into(),
            error: "no versionInfo in install_profile".into(),
        })?;
    let text = serde_json::to_string(version_info).map_err(|e| InstallError {
        stage: "forge-legacy".into(),
        error: format!("re-serialise: {e}"),
    })?;
    let mut child = parse_version_json(&text).map_err(|e| InstallError {
        stage: "forge-legacy".into(),
        error: format!("parse: {e}"),
    })?;
    child.id = combo.synth_id();
    let vanilla = fetch_vanilla_details(&combo.mc).await?;
    child.inherits_from = Some(vanilla.id.clone());
    let merged = merge_inherits(child, vanilla);

    // Step 3: Download libraries. The `forge-<v>.jar` (no-classifier) entry is
    // URL-less in the merged profile but we placed it on disk above; the
    // download loop's existence-check + TOFU treatment skips it.
    download_vanilla_payload(&merged, &combo.synth_id(), paths).await?;
    Ok(merged)
}

async fn install_forge_processor_era(
    combo: &Combo,
    paths: &Paths,
    installer_bytes: &[u8],
    profile_value: &serde_json::Value,
    _is_modern: bool,
) -> Result<VersionDetails, InstallError> {
    use lucerna_lib::forge::installer::transitional::{
        classpath_coords_to_libraries, parse_install_profile, substitute_args,
    };

    let raw_text = serde_json::to_string(profile_value).unwrap();
    let profile = parse_install_profile(&raw_text).map_err(|e| InstallError {
        stage: "forge-install-profile-v2".into(),
        error: format!("{e:?}"),
    })?;
    let version_details = {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| {
            InstallError {
                stage: "forge-version-json".into(),
                error: format!("zip: {e}"),
            }
        })?;
        let mut entry = zip.by_name("version.json").map_err(|e| InstallError {
            stage: "forge-version-json".into(),
            error: format!("{e}"),
        })?;
        let mut s = String::new();
        entry.read_to_string(&mut s).map_err(|e| InstallError {
            stage: "forge-version-json".into(),
            error: format!("read: {e}"),
        })?;
        parse_version_json(&s).map_err(|e| InstallError {
            stage: "forge-version-json".into(),
            error: format!("parse: {e}"),
        })?
    };

    // Vanilla MC client must exist (binarypatcher consumes it).
    let vanilla = fetch_vanilla_details(&combo.mc).await?;
    download_vanilla_payload(&vanilla, &combo.mc, paths).await?;

    // Locate Java for the install processors. Use the MC-matched JRE (same
    // as launch) so binarypatcher 1.2.0 / FART runs on the right major
    // version. Fall back to system Java if no java_version is declared
    // (legacy MC < 1.7 → jre-legacy = Java 8, sufficient for old binarypatcher).
    let component = vanilla
        .java_version
        .as_ref()
        .map(|jv| jv.component.as_str())
        .unwrap_or(lucerna_lib::jre::DEFAULT_LEGACY_COMPONENT);
    let java_bin = ensure_jre_into(component, paths).await.or_else(|_| {
        locate_system_java().ok_or_else(|| InstallError {
            stage: "forge-java".into(),
            error: "no JRE installable + no system Java".into(),
        })
    })?;

    let cache_dir = paths.forge_cache(&combo.mc, &combo.loader_version);
    tokio::fs::create_dir_all(&cache_dir).await.ok();
    let cache_dir_str = cache_dir.display().to_string();
    let installer_path_str = paths
        .installer_jar(&combo.mc, &combo.loader_version)
        .display()
        .to_string();
    let minecraft_jar = paths.vanilla_client_jar(&combo.mc).display().to_string();
    let libs_root = paths.libraries();

    extract_installer_maven_tree(installer_bytes, &libs_root).await?;

    // Fetch install_profile.libraries[] (filter URL-less; they ship in the maven/ subtree).
    let os = detect_os();
    let arch = detect_arch();
    for lib in &profile.libraries {
        let has_url = lib.url.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
            || lib
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .map(|a| !a.url.is_empty())
                .unwrap_or(false);
        if !has_url {
            continue;
        }
        for (rel, url, sha1, _size) in artifacts_to_install(lib, os, arch) {
            if url.is_empty() {
                continue;
            }
            download_file(&url, &libs_root.join(&rel), &sha1).await?;
        }
    }

    // Run client-side processors.
    for (i, p) in profile.processors.iter().enumerate() {
        if let Some(sides) = &p.sides {
            if !sides.iter().any(|s| s == "client") {
                continue;
            }
        }
        let cp_libs = classpath_coords_to_libraries(&p.classpath);
        for lib in &cp_libs {
            for (rel, url, sha1, _size) in artifacts_to_install(lib, os, arch) {
                if url.is_empty() {
                    continue;
                }
                download_file(&url, &libs_root.join(&rel), &sha1).await?;
            }
        }
        let resolved = substitute_args(
            &p.args,
            &profile.data,
            "client",
            &libs_root,
            installer_bytes,
            &installer_path_str,
            &cache_dir_str,
            &minecraft_jar,
        )
        .await
        .map_err(|e| InstallError {
            stage: format!("forge-substitute-args[{i}]"),
            error: format!("{e:?}"),
        })?;
        let mut cp_paths: Vec<PathBuf> = p
            .classpath
            .iter()
            .filter_map(|c| maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();
        if let Some(rel) = maven_coord_to_relative_path(&p.jar) {
            cp_paths.push(libs_root.join(rel));
        }
        let ctx = ProcessorContext {
            classpath: cp_paths,
            cache_dir: cache_dir.clone(),
            java_bin: Some(java_bin.clone()),
        };
        run_processor(&p.jar, resolved, &ctx)
            .await
            .map_err(|e| InstallError {
                stage: format!("forge-processor[{i}]:{}", p.jar),
                error: format!("{e:?}"),
            })?;
    }

    // Use the same merge as production launch path.
    let mut child = version_details;
    child.id = combo.synth_id();
    child.inherits_from = Some(combo.mc.clone());
    let merged = merge_inherits(child, vanilla);

    download_vanilla_payload(&merged, &combo.synth_id(), paths).await?;
    Ok(merged)
}

async fn extract_installer_maven_tree(
    installer_bytes: &[u8],
    libs_root: &Path,
) -> Result<(), InstallError> {
    use std::io::Read;
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| InstallError {
            stage: "forge-maven-tree".into(),
            error: format!("{e}"),
        })?;
    let mut to_write: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| InstallError {
            stage: "forge-maven-tree".into(),
            error: format!("index {i}: {e}"),
        })?;
        let name = entry.name().to_string();
        if !name.starts_with("maven/") || name.ends_with('/') {
            continue;
        }
        let rel = name.strip_prefix("maven/").unwrap();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| InstallError {
            stage: "forge-maven-tree".into(),
            error: format!("read {name}: {e}"),
        })?;
        to_write.push((libs_root.join(rel), buf));
    }
    for (dest, bytes) in to_write {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(&dest, &bytes)
            .await
            .map_err(|e| InstallError {
                stage: "forge-maven-tree".into(),
                error: format!("write {}: {e}", dest.display()),
            })?;
    }
    Ok(())
}

// ─── Launch + watch ─────────────────────────────────────────────────────────

const MENU_MARKERS: &[&str] = &[
    "Sound engine started",
    "Setting user:",
    "[Render thread/INFO]: Stopping!", // covers ultra-fast graceful exits
];

async fn launch_and_watch(
    combo: &Combo,
    paths: &Paths,
    details: &VersionDetails,
    install_at: Duration,
    combo_log: &Path,
) -> Outcome {
    let synth = combo.synth_id();
    let game_dir = paths.game_dir(&synth);
    let natives_dir = paths.natives_dir(&synth);
    tokio::fs::create_dir_all(&game_dir).await.ok();
    tokio::fs::create_dir_all(&natives_dir).await.ok();
    // Remove any leftover MC `latest.log` from a prior run — otherwise our
    // marker watcher sees the previous run's "Sound engine started" and
    // declares instant success without waiting for the new run.
    let _ = tokio::fs::remove_file(game_dir.join("logs").join("latest.log")).await;

    // Extract `*-natives-<os>.jar` classifier jars into natives_dir. Real
    // `launch::start` does this; vanilla legacy MC (1.7.10, 1.12.2) loads
    // lwjgl/jinput DLLs from `-Djava.library.path` and crashes without them.
    {
        let os_ = detect_os();
        let arch_ = detect_arch();
        if let Err(e) = lucerna_lib::launch::natives::extract_natives(
            &details.libraries,
            &paths.libraries(),
            &natives_dir,
            os_,
            arch_,
        )
        .await
        {
            return Outcome::LaunchSpawnFailed {
                error: format!("extract_natives: {e:?}"),
            };
        }
    }

    // Pick the JRE component declared by the merged version JSON. For
    // legacy MC (< 1.7) the field is absent and we fall back to Mojang's
    // jre-legacy (Java 8).
    let component = details
        .java_version
        .as_ref()
        .map(|jv| jv.component.as_str())
        .unwrap_or(lucerna_lib::jre::DEFAULT_LEGACY_COMPONENT);
    let java = match ensure_jre_into(component, paths).await {
        Ok(j) => j,
        Err(e) => {
            return Outcome::LaunchSpawnFailed {
                error: format!("ensure_jre({component}): {} — {}", e.stage, e.error),
            }
        }
    };
    let os = detect_os();
    let arch = detect_arch();

    // Synthesize an offline account for the launcher's substitution.
    let account = lucerna_lib::accounts::Account {
        id: "matrix-tester".into(),
        name: "MatrixTester".into(),
        uuid: "00000000-0000-0000-0000-000000000000".into(),
        expires_at: None,
        kind: lucerna_lib::accounts::AccountKind::Offline,
    };

    // Mirror production spawn.rs: Forge/NeoForge ship a patched MC in
    // libraries and the loader's own MinecraftLocator finds it; adding the
    // vanilla jar to the classpath duplicates net.minecraft.* and triggers
    // a JPMS ResolutionException on the BootstrapLauncher / ForgeBootstrap
    // module-path. Vanilla/Fabric/Quilt do need it on the classpath.
    let client_jar = match combo.loader {
        MatrixLoader::Real(Loader::Forge) | MatrixLoader::Real(Loader::NeoForge) => None,
        _ => Some(paths.versions().join(&synth).join(format!("{synth}.jar"))),
    };
    let argv_input = lucerna_lib::launch::args::ArgvInput {
        details,
        account: &account,
        java_path: java.clone(),
        libraries_dir: paths.libraries(),
        assets_dir: paths.assets(),
        natives_dir: natives_dir.clone(),
        game_dir: game_dir.clone(),
        client_jar,
        os,
        arch,
        quick_play: None,
    };
    let manifest_argv = match lucerna_lib::launch::args::build_argv(&argv_input) {
        Ok(a) => a,
        Err(e) => {
            return Outcome::LaunchSpawnFailed {
                error: format!("build_argv: {e:?}"),
            }
        }
    };
    // Prepend `-Xmx2048m` so MC has enough heap. UI's `launch::spawn` does
    // this from `instance.max_heap_mb`; we hardcode the default for the matrix.
    let mut argv: Vec<String> = vec!["-Xmx2048m".to_string()];
    argv.extend(manifest_argv);

    // Redirect child stdout+stderr to a file so we can read it on failure.
    // The pipes would otherwise back-pressure and block the child once full.
    let stdio_log_path = combo_log.with_extension("stdio.log");
    let stdio_file = match std::fs::File::create(&stdio_log_path) {
        Ok(f) => f,
        Err(e) => {
            return Outcome::LaunchSpawnFailed {
                error: format!("create stdio log: {e}"),
            }
        }
    };
    let stdio_file2 = match stdio_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return Outcome::LaunchSpawnFailed {
                error: format!("clone stdio fd: {e}"),
            }
        }
    };

    let mut cmd = Command::new(&java);
    cmd.args(&argv);
    cmd.current_dir(&game_dir);
    cmd.stdout(Stdio::from(stdio_file));
    cmd.stderr(Stdio::from(stdio_file2));
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Outcome::LaunchSpawnFailed {
                error: format!("spawn: {e}"),
            }
        }
    };

    // Watch loop: poll latest.log + child exit status, up to 120s.
    let watch_start = Instant::now();
    let timeout = Duration::from_secs(120);
    let latest_log = game_dir.join("logs").join("latest.log");

    loop {
        // Check exit.
        match child.try_wait() {
            Ok(Some(status)) => {
                let mc_tail = read_log_tail(&latest_log).await;
                let stdio_tail = read_log_tail(&stdio_log_path).await;
                let combo_text = format!(
                    "EXIT {status:?}\nargv:\n  {}\nMC latest.log tail:\n{mc_tail}\n\nJVM stdout/stderr tail:\n{stdio_tail}\n",
                    argv.join(" ")
                );
                let _ = std::fs::write(combo_log, combo_text);
                return Outcome::LaunchCrashed {
                    exit_code: status.code(),
                    log_tail: format!("{mc_tail}\n--- jvm stdio ---\n{stdio_tail}"),
                };
            }
            Ok(None) => {}
            Err(e) => {
                return Outcome::LaunchSpawnFailed {
                    error: format!("try_wait: {e}"),
                }
            }
        }

        // Check menu marker (look in both MC's latest.log and JVM stdio).
        let mc_log = tokio::fs::read_to_string(&latest_log).await.ok();
        let stdio_log = tokio::fs::read_to_string(&stdio_log_path).await.ok();
        let combined = match (mc_log.as_deref(), stdio_log.as_deref()) {
            (Some(a), Some(b)) => format!("{a}\n{b}"),
            (Some(a), None) => a.to_string(),
            (None, Some(b)) => b.to_string(),
            (None, None) => String::new(),
        };
        if !combined.is_empty() && MENU_MARKERS.iter().any(|m| combined.contains(m)) {
            let _ = child.kill().await;
            let _ = std::fs::write(combo_log, format!("SUCCESS\nargv:\n  {}\n", argv.join(" ")));
            return Outcome::Success {
                menu_at: watch_start.elapsed(),
                install_at,
            };
        }

        if watch_start.elapsed() > timeout {
            let _ = child.kill().await;
            let mc_tail = read_log_tail(&latest_log).await;
            let stdio_tail = read_log_tail(&stdio_log_path).await;
            let _ = std::fs::write(
                combo_log,
                format!(
                    "TIMEOUT after 120s\nargv:\n  {}\nMC latest.log tail:\n{mc_tail}\n\nJVM stdio tail:\n{stdio_tail}\n",
                    argv.join(" ")
                ),
            );
            return Outcome::LaunchTimedOut {
                log_tail: format!("{mc_tail}\n--- jvm stdio ---\n{stdio_tail}"),
            };
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn read_log_tail(path: &Path) -> String {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(40);
            lines[start..].join("\n")
        }
        Err(_) => "(latest.log absent)".into(),
    }
}

// ─── OS / Java helpers ──────────────────────────────────────────────────────

fn detect_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}
fn detect_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86"
    }
}

fn locate_system_java() -> Option<PathBuf> {
    // Prefer JAVA_HOME, fall back to system PATH (`java` resolves).
    // Used for Forge install processors (FART/SpecialSource/binarypatcher 1.2.0+),
    // which are agnostic to which JRE major version they run on. Launch itself
    // uses `ensure_jre_into` to install the MC-version-correct JRE.
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let exe = if cfg!(windows) { "java.exe" } else { "java" };
        let p = PathBuf::from(home).join("bin").join(exe);
        if p.exists() {
            return Some(p);
        }
    }
    Some(PathBuf::from("java"))
}

/// Install (or use cached) Mojang JRE for the given component, return path
/// to the `java` executable. Replicates `jre::install::ensure_jre` for the
/// matrix harness (which doesn't have an AppHandle).
async fn ensure_jre_into(component: &str, paths: &Paths) -> Result<PathBuf, InstallError> {
    use lucerna_lib::jre::manifest::{
        fetch_component_manifest, fetch_top_level, mojang_platform_key, pick_component, FileEntry,
    };
    let os = detect_os();
    let arch = detect_arch();
    let platform_key = mojang_platform_key(os, arch).map_err(|e| InstallError {
        stage: "jre-platform".into(),
        error: format!("{e:?}"),
    })?;
    let top = fetch_top_level().await.map_err(|e| InstallError {
        stage: "jre-top-manifest".into(),
        error: format!("{e:?}"),
    })?;
    let comp_ref = pick_component(&top, platform_key, component).map_err(|e| InstallError {
        stage: "jre-pick-component".into(),
        error: format!("{e:?}"),
    })?;
    let comp_root = paths.root.join("jres").join(component);
    let marker = comp_root.join(".installed");

    // Marker fast-path.
    let expected_marker = format!("{}\n{}\n", comp_ref.version.name, comp_ref.manifest.sha1);
    if let Ok(raw) = tokio::fs::read_to_string(&marker).await {
        if raw == expected_marker {
            return Ok(jre_exe_path(&comp_root));
        }
    }
    let _ = tokio::fs::remove_dir_all(&comp_root).await;
    let comp_manifest = fetch_component_manifest(&comp_ref)
        .await
        .map_err(|e| InstallError {
            stage: "jre-component-manifest".into(),
            error: format!("{e:?}"),
        })?;
    for (rel, entry) in &comp_manifest.files {
        let dest = comp_root.join(rel);
        match entry {
            FileEntry::Directory {} => {
                tokio::fs::create_dir_all(&dest)
                    .await
                    .map_err(|e| InstallError {
                        stage: "jre-mkdir".into(),
                        error: format!("{}: {e}", dest.display()),
                    })?;
            }
            FileEntry::Link { .. } => {
                return Err(InstallError {
                    stage: "jre-link".into(),
                    error: "link entry in JRE manifest not supported on Windows".into(),
                });
            }
            FileEntry::File { downloads, .. } => {
                download_file(&downloads.raw.url, &dest, &downloads.raw.sha1).await?;
            }
        }
    }
    tokio::fs::write(&marker, &expected_marker)
        .await
        .map_err(|e| InstallError {
            stage: "jre-marker".into(),
            error: format!("{}: {e}", marker.display()),
        })?;
    Ok(jre_exe_path(&comp_root))
}

fn jre_exe_path(comp_root: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "javaw.exe" } else { "java" };
    comp_root.join("bin").join(exe)
}

// ─── Reporting ──────────────────────────────────────────────────────────────

fn print_summary_table(results: &[(Combo, Outcome)]) {
    eprintln!("\n| MC | Loader | Version | Outcome |");
    eprintln!("|---|---|---|---|");
    for (c, o) in results {
        let v = if c.loader == MatrixLoader::Vanilla {
            "—".into()
        } else {
            c.loader_version.clone()
        };
        eprintln!(
            "| {} | {} | {} | {} |",
            c.mc,
            c.loader.as_label(),
            v,
            o.summary()
        );
    }
    let ok = results.iter().filter(|(_, o)| o.is_success()).count();
    let fail = results.len() - ok;
    eprintln!("\nTotals: {ok} ok, {fail} failed (of {})", results.len());
}

// ─── Test entry ─────────────────────────────────────────────────────────────

fn data_root() -> PathBuf {
    let base = std::env::temp_dir().join("lucerna-matrix-data");
    std::fs::create_dir_all(&base).expect("create matrix data root");
    base
}

fn log_root() -> PathBuf {
    let crate_root = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let dir = crate_root.join("target").join("loader-matrix-logs");
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir).expect("create matrix log root");
    dir
}

async fn build_combos() -> Vec<Combo> {
    let mut out = Vec::new();
    let mcs = mc_versions();
    for mc_owned in &mcs {
        let mc = mc_owned.as_str();
        for loader in loaders_for(mc) {
            match loader {
                MatrixLoader::Vanilla => out.push(Combo {
                    mc: mc.to_string(),
                    loader,
                    loader_version: String::new(),
                }),
                MatrixLoader::Real(real) => match list_loaders(real, mc).await {
                    Ok(versions) => match versions.iter().find(|v| v.stable) {
                        Some(rec) => out.push(Combo {
                            mc: mc.to_string(),
                            loader,
                            loader_version: rec.version.clone(),
                        }),
                        None => eprintln!(
                            "  SKIP {mc}/{}: no recommended version in meta",
                            loader.as_label()
                        ),
                    },
                    Err(e) => eprintln!("  SKIP {mc}/{}: meta error: {e:?}", loader.as_label()),
                },
            }
        }
    }
    out
}

#[tokio::test]
#[ignore]
async fn loader_matrix_e2e() {
    let data = data_root();
    let logs = log_root();
    let paths = Paths { root: data.clone() };

    eprintln!("=== Loader matrix e2e ===");
    eprintln!("data root: {}", data.display());
    eprintln!("log root:  {}", logs.display());

    let combos = build_combos().await;
    eprintln!("matrix size: {}", combos.len());

    let mut results: Vec<(Combo, Outcome)> = Vec::with_capacity(combos.len());
    let total_start = Instant::now();

    for (i, combo) in combos.iter().enumerate() {
        eprintln!("\n[{}/{}] {}", i + 1, combos.len(), combo.label());
        let combo_log = logs.join(format!("{}.log", combo.log_filename()));
        let install_start = Instant::now();
        let details = match combo.loader {
            MatrixLoader::Vanilla => install_vanilla(&combo.mc, &paths).await,
            MatrixLoader::Real(Loader::Fabric) | MatrixLoader::Real(Loader::Quilt) => {
                install_fabric_or_quilt(combo, &paths).await
            }
            MatrixLoader::Real(Loader::Forge) => install_forge(combo, &paths).await,
            MatrixLoader::Real(Loader::NeoForge) => install_neoforge(combo, &paths).await,
        };
        let outcome = match details {
            Ok(d) => {
                let install_at = install_start.elapsed();
                launch_and_watch(combo, &paths, &d, install_at, &combo_log).await
            }
            Err(e) => {
                // Write the FULL error (un-truncated) to the combo log so we
                // can actually diagnose. `outcome.summary()` truncates to 160
                // chars for the in-process console table, which loses the
                // tail of long URLs / nested error formats.
                let _ = std::fs::write(
                    &combo_log,
                    format!("INSTALL FAILED\nstage: {}\nerror:\n{}\n", e.stage, e.error),
                );
                Outcome::InstallFailed {
                    stage: e.stage,
                    error: e.error,
                }
            }
        };
        eprintln!("  → {}", outcome.summary());
        results.push((combo.clone(), outcome));
    }

    eprintln!("\n=== Summary (total {:?}) ===", total_start.elapsed());
    print_summary_table(&results);

    let failed: Vec<_> = results.iter().filter(|(_, o)| !o.is_success()).collect();
    if !failed.is_empty() {
        panic!(
            "{} of {} combos failed (see per-combo logs in {})",
            failed.len(),
            results.len(),
            logs.display()
        );
    }
}
