use specta::Type;

use crate::mods::curseforge::keyring as cf_keyring;
use crate::mods::curseforge::CurseForgeClient;
use crate::mods::deps::{FetchedDeps, ProjectKey, ResolvedNode};
use crate::mods::modpack;
use crate::mods::modpack::schema::{
    ModpackHit, ModpackProgress, ModpackSearchPage, ModpackSort, ModpackStatus, ModpackSummary,
};
use crate::mods::modrinth::ModrinthClient;
use crate::mods::platform::*;
use serde::Serialize;
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri_specta::Event;

// =========================================================================
// Sub-modules (domain splits) — each file holds only #[tauri::command] fns
// =========================================================================

mod accounts;
pub use accounts::*;
mod versions;
pub use versions::*;
mod instances;
pub use instances::*;
mod logs;
pub use logs::*;
mod journal;
pub use journal::*;
mod reports;
pub use reports::*;
mod worlds;
pub use worlds::*;
mod servers;
pub use servers::*;
mod playtime;
pub use playtime::*;
mod mods;
pub use mods::*;
mod assets;
pub use assets::*;
mod curseforge_key;
pub use curseforge_key::*;
mod modpack_cmds;
pub use modpack_cmds::*;
mod instance_import_cmds;
pub use instance_import_cmds::*;
mod settings;
pub use settings::*;
mod update;
pub use update::*;
mod window;
pub use window::*;
mod servers_runtime;
pub use servers_runtime::*;
mod data_location;
pub use data_location::*;
mod screenshots;
pub use screenshots::*;
mod clipboard;
pub use clipboard::*;
mod cosmetics;
pub use cosmetics::*;
mod skin_library;
pub use skin_library::*;
mod desktop;
pub use desktop::*;
mod datapacks;
pub use datapacks::*;
mod l10n;
pub use l10n::*;

// =========================================================================
// Types kept here (referenced by lib.rs via `commands::` paths)
// =========================================================================

#[derive(Debug, serde::Serialize, Type)]
pub struct Greeting {
    pub message: String,
}

const MAX_INSTANCE_NAME_LEN: u32 = 32;

// =========================================================================
// Helper functions (kept in mod.rs so domain files can access via super::*)
// =========================================================================

/// Validate instance name at the IPC boundary.
///
/// Reasons live as typed Error variants so the UI doesn't string-parse.
/// Count uses unicode scalar values (chars), not bytes — a 32-char
/// cyrillic name is 64 bytes but 32 graphemes, and that's fine.
fn validate_instance_name(name: &str) -> Result<(), crate::error::Error> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::error::Error::InstanceNameEmpty);
    }
    let count = trimmed.chars().count() as u32;
    if count > MAX_INSTANCE_NAME_LEN {
        return Err(crate::error::Error::InstanceNameTooLong {
            max: MAX_INSTANCE_NAME_LEN,
            actual: count,
        });
    }
    Ok(())
}

/// Shared prelude for install_instance and launch_instance: confirm the
/// instance exists, read its JSON, and resolve the effective version id.
/// Returns the version id only; callers that need the full Instance read
/// it again (cheap; same file on disk).
fn resolve_instance_effective_id(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<String, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == instance_id) {
        return Err(crate::error::Error::InstanceNotFound {
            id: instance_id.to_string(),
        });
    }
    let json_path = crate::paths::instance_json(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    crate::instances::status::effective_version_id(&instance)
        .ok_or(crate::error::Error::NoVersionSelected)
}

/// Persist a `VerifyReport` summary into the instance's `instance.json` so the
/// UI can surface a passive integrity badge + Overview row without re-hashing.
/// Read-modify-write — preserves every other field. Timestamp = now (unix ms).
fn persist_integrity(
    app: &tauri::AppHandle,
    instance_id: &str,
    report: &crate::verify::VerifyReport,
) -> Result<(), crate::error::Error> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let path = crate::paths::instance_json(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let mut file = crate::instances::store::read_instance_json(&path)?;
    file.integrity = Some(crate::verify::IntegrityStatus::from_report(report, now_ms));
    crate::instances::store::write_instance_json(&path, &file)
}

/// For each conflict candidate that has platform identity, query its
/// newest version for the instance's MC+loader and, if one exists, fill
/// `swap_target`/`swap_version_label`. Best-effort, sequential, bounded
/// by the small candidate count.
async fn enrich_swap_targets(
    candidates: &mut [crate::logs::diagnose::repair::ConflictCandidate],
    installed: &[crate::mods::platform::InstalledMod],
    instance: &crate::instances::schema::InstanceFile,
) {
    use crate::mods::platform::VersionRef;
    for c in candidates.iter_mut() {
        let Some(m) = installed.iter().find(|m| m.sha1 == c.sha1) else {
            continue;
        };
        let (Some(source), Some(project_id)) = (m.source, m.project_id.as_deref()) else {
            continue;
        };
        let platform = platform_for(source);
        let versions = platform
            .versions(
                project_id,
                Some(&instance.mc_version),
                Some(instance.loader),
            )
            .await;
        if let Ok(list) = versions {
            if let Some(v) = list.into_iter().next() {
                // Don't offer a "swap" to the version that's already installed
                // — that would be a no-op ("Update to <same version>").
                let already_installed = m.version_id.as_deref() == Some(v.version_id.as_str())
                    || m.version_number.as_deref() == Some(v.version_number.as_str());
                if already_installed {
                    continue;
                }
                c.swap_version_label = Some(v.version_number.clone());
                c.swap_target = Some(VersionRef {
                    source,
                    project_id: project_id.to_string(),
                    version_id: v.version_id,
                });
            }
        }
    }
}

fn platform_for(source: ModSource) -> Box<dyn ModPlatform> {
    match source {
        ModSource::Modrinth => Box::new(ModrinthClient::new()),
        ModSource::Curseforge => Box::new(CurseForgeClient::new()),
        // FTB is a modpack-only source — no per-mod browser.
        ModSource::Ftb => Box::new(crate::mods::unsupported::UnsupportedModPlatform {
            source: ModSource::Ftb,
        }),
        // ATLauncher is a modpack-only source — no per-mod browser.
        ModSource::Atlauncher => Box::new(crate::mods::unsupported::UnsupportedModPlatform {
            source: ModSource::Atlauncher,
        }),
        ModSource::Hangar => Box::new(crate::mods::hangar::HangarClient::new()),
    }
}

// =========================================================================
// Transitive-dependency resolver adapter helpers
// =========================================================================

/// Backend loader-project slugs (mirrors the frontend LOADER_SLUGS in
/// ModBrowseView.svelte). A dep whose project slug is one of these is a
/// loader — managed at the instance level, never installed as a mod jar.
const LOADER_SLUGS: &[&str] = &[
    "neoforge",
    "forge",
    "fabric",
    "fabric-loader",
    "quilt",
    "quilt-loader",
    "minecraft",
];

/// Is `version`'s project a loader? Looks up the project slug, memoized in
/// `loader_cache`. One `project()` call per distinct project, amortized.
/// Fails open: an un-classifiable project is treated as a normal mod.
///
/// Takes the mutex by reference and releases the lock *before* the network
/// call so that concurrent `join_all` invocations from `fetch_one_level` can
/// proceed in parallel without each other blocking on the cache lock.
async fn is_loader_project(
    platform: &dyn ModPlatform,
    loader_cache: &tokio::sync::Mutex<std::collections::HashMap<ProjectKey, bool>>,
    v: &ModVersion,
) -> bool {
    let key = ProjectKey::of_version(v);
    {
        let cache = loader_cache.lock().await;
        if let Some(hit) = cache.get(&key) {
            return *hit;
        }
    } // lock released before the network call
    let is_loader = match platform.project(&v.project_id).await {
        Ok(p) => p
            .summary
            .slug
            .as_deref()
            .map(|s| LOADER_SLUGS.contains(&s.to_ascii_lowercase().as_str()))
            .unwrap_or(false),
        Err(_) => false,
    };
    loader_cache.lock().await.insert(key, is_loader);
    is_loader
}

/// Build `FetchedDeps` for one version: call the platform's one-level
/// `resolve_deps` and classify each resolved dep as loader / normal.
///
/// Loader classification for all deps at this level is done concurrently via
/// `join_all` — each `is_loader_project` call is an independent project()
/// lookup memoized in the shared cache. The cache lock is held only for the
/// brief read/write, not across the network call, so the futures can
/// genuinely run in parallel.
async fn fetch_one_level(
    platform: &dyn ModPlatform,
    loader_cache: &tokio::sync::Mutex<std::collections::HashMap<ProjectKey, bool>>,
    v: &ModVersion,
    mc: &str,
    loader: LoaderKind,
) -> Result<FetchedDeps, crate::error::Error> {
    let rd = platform.resolve_deps(v, mc, loader).await?;
    // Classify all deps' loader-ness concurrently (each is an independent
    // project() lookup, memoized in the shared cache).
    let req_flags = futures_util::future::join_all(
        rd.required
            .iter()
            .map(|r| is_loader_project(platform, loader_cache, &r.version)),
    )
    .await;
    let opt_flags = futures_util::future::join_all(
        rd.optional
            .iter()
            .map(|o| is_loader_project(platform, loader_cache, &o.version)),
    )
    .await;
    let required = rd
        .required
        .into_iter()
        .zip(req_flags)
        .map(|(r, is_loader)| ResolvedNode {
            version: r.version,
            is_loader,
            selection_reason: r.selection_reason,
        })
        .collect();
    let optional = rd
        .optional
        .into_iter()
        .zip(opt_flags)
        .map(|(o, is_loader)| ResolvedNode {
            version: o.version,
            is_loader,
            selection_reason: o.selection_reason,
        })
        .collect();
    Ok(FetchedDeps {
        required,
        optional,
        incompatible: rd.incompatible,
        unresolvable: rd.unresolvable,
    })
}

// =========================================================================
// Mod event types (referenced by lib.rs via collect_events!)
// =========================================================================

/// Streamed progress for a single mod install operation. Tagged union so
/// the UI can switch on `phase` and show a progress bar / spinner.
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ModInstallProgress {
    Downloading {
        instance_id: String,
        project_id: String,
        /// f64 not u64 — specta forbids BigInt-style exports.
        bytes_done: f64,
        bytes_total: Option<f64>,
        /// 1-based index of the jar being installed within this operation.
        current: u32,
        /// Total jars this operation will install. `0` while the set is still
        /// being resolved — manifest extras download before `install_seq`
        /// exists, so their ticks genuinely have no total yet.
        total: u32,
    },
    Verifying {
        instance_id: String,
        project_id: String,
        bytes_done: f64,
        /// 1-based index of the jar being installed within this operation.
        current: u32,
        /// Total jars this operation will install. `0` while the set is still
        /// being resolved — manifest extras download before `install_seq`
        /// exists, so their ticks genuinely have no total yet.
        total: u32,
    },
    Copying {
        instance_id: String,
        project_id: String,
        /// 1-based index of the jar being installed within this operation.
        /// Copying is phase 2 (commit); by the time it runs, phase 1 has
        /// already advanced `current` through every item, so this always
        /// equals `total` — see `install_batch` / `update_one`.
        current: u32,
        /// Total jars this operation will install. `0` while the set is still
        /// being resolved — manifest extras download before `install_seq`
        /// exists, so their ticks genuinely have no total yet.
        total: u32,
    },
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModInstalled {
    pub instance_id: String,
    pub sha1: String,
    pub filename: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModUninstalled {
    pub instance_id: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModToggle {
    pub instance_id: String,
    pub sha1: String,
    /// True iff the mod is now enabled. UI uses this to drive the toggle
    /// switch without re-querying `mods_list_installed`.
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModInstallFailed {
    pub instance_id: String,
    pub project_id: String,
    pub error: crate::error::Error,
}

/// Emitted when a freshly-installed JRE was stamped with a non-Auto GPU
/// preference, so the UI can surface a one-time "applied to new runtime" toast.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct GpuPrefApplied {
    /// The preference applied (serialized snake_case: "high_performance" | "power_saving").
    pub preference: crate::instances::schema::GpuPreference,
    /// GPU name the preference resolves to (e.g. "NVIDIA GeForce RTX 3050 Ti"), if known.
    pub gpu_name: Option<String>,
}

/// Per-instance root, e.g. `<app_data>/instances/<id>/`. The mod install
/// pipeline writes under `{root}/.minecraft/mods/` and tracks state in
/// `{root}/lucerna/installed-mods.json`.
///
/// `pub(crate)` for `l10n::apply`, which was lifted out of `l10n_apply` and
/// resolves the same instance the same way.
pub(crate) fn instance_root(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<PathBuf, crate::error::Error> {
    crate::paths::instance_dir(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_dir>", e))
}

/// Launcher app-data directory — host of the shared mod cache.
fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, crate::error::Error> {
    crate::paths::app_dir(app).map_err(|e| crate::error::Error::io("<app_dir>", e))
}

/// Read the active MC version + loader for an instance from
/// `instance.json`. Returns `InstanceNotFound` if the file is missing.
///
/// `pub(crate)` for `l10n::apply`: the pack rebuild moved out of
/// `l10n_apply`, and a second copy of this existence check would be free to
/// drift from the one every other instance-scoped command uses.
pub(crate) fn read_active_mc_and_loader(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<(String, LoaderKind), crate::error::Error> {
    let all = crate::instances::list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == instance_id) {
        return Err(crate::error::Error::InstanceNotFound {
            id: instance_id.to_string(),
        });
    }
    let json_path = crate::paths::instance_json(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    Ok((instance.mc_version, instance.loader))
}

/// Resolve a `VersionRef` to a full `ModVersion` by querying the platform
/// for the project's available versions (filtered by MC + loader).
async fn find_version(
    platform: &mut Box<dyn ModPlatform>,
    vr: &VersionRef,
    mc: &str,
    loader: LoaderKind,
) -> crate::error::Result<ModVersion> {
    let vs = platform
        .versions(&vr.project_id, Some(mc), Some(loader))
        .await?;
    vs.into_iter()
        .find(|v| v.version_id == vr.version_id)
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: match vr.source {
                ModSource::Modrinth => "modrinth",
                ModSource::Curseforge => "curseforge",
                ModSource::Ftb => "ftb", // FTB: pack-managed, not individually resolvable.
                ModSource::Atlauncher => "atlauncher", // ATLauncher: pack-managed, not individually resolvable.
                ModSource::Hangar => "hangar",
            }
            .into(),
        })
}

fn version_matches(v: &ModVersion, vr: &VersionRef) -> bool {
    v.source == vr.source && v.project_id == vr.project_id && v.version_id == vr.version_id
}

fn dedup_versions(
    it: impl Iterator<Item = crate::mods::platform::ModVersion>,
) -> Vec<crate::mods::platform::ModVersion> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in it {
        if seen.insert(crate::mods::deps::ProjectKey::of_version(&v)) {
            out.push(v);
        }
    }
    out
}

/// Dedup planned deps by source-specific ProjectKey, preserving first-seen order.
fn dedup_planned(
    items: impl Iterator<Item = crate::mods::platform::PlannedDep>,
) -> Vec<crate::mods::platform::PlannedDep> {
    use crate::mods::deps::ProjectKey;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in items {
        if seen.insert(ProjectKey::of_version(&p.version)) {
            out.push(p);
        }
    }
    out
}

fn version_to_ref(v: &crate::mods::platform::ModVersion) -> crate::mods::platform::DepProjectRef {
    match v.source {
        crate::mods::platform::ModSource::Modrinth => {
            crate::mods::platform::DepProjectRef::Modrinth {
                project_id: v.project_id.clone(),
                version_id: Some(v.version_id.clone()),
            }
        }
        crate::mods::platform::ModSource::Curseforge => {
            crate::mods::platform::DepProjectRef::Curseforge {
                mod_id: v.project_id.parse().unwrap_or(0),
                file_id: v.version_id.parse().ok(),
            }
        }
        // TODO(ftb): placeholder — FTB versions are dead in this path today (no FTB mod browser /
        // dep resolution). If a future task makes FTB mods enter dedup/dep-graph keying, introduce
        // DepProjectRef::Ftb instead of borrowing the Modrinth tag, to avoid a numeric-id collision
        // with real Modrinth ids.
        crate::mods::platform::ModSource::Ftb => crate::mods::platform::DepProjectRef::Modrinth {
            project_id: v.project_id.clone(),
            version_id: Some(v.version_id.clone()),
        },
        // TODO(atlauncher): placeholder — ATLauncher versions are dead in this path today.
        crate::mods::platform::ModSource::Atlauncher => {
            crate::mods::platform::DepProjectRef::Modrinth {
                project_id: v.project_id.clone(),
                version_id: Some(v.version_id.clone()),
            }
        }
        // TODO(hangar): placeholder — the Hangar client ships no dependency links (its
        // versions carry empty `deps` and resolve_deps returns an empty plan), so Hangar
        // versions stay dead in this path. Borrowing the Modrinth tag mirrors the
        // FTB/ATLauncher stubs above; introduce DepProjectRef::Hangar before Hangar plugins
        // can reach dedup/dep-graph keying, to avoid a collision with real Modrinth ids.
        crate::mods::platform::ModSource::Hangar => {
            crate::mods::platform::DepProjectRef::Modrinth {
                project_id: v.project_id.clone(),
                version_id: Some(v.version_id.clone()),
            }
        }
    }
}

// =========================================================================
// Modpack helpers (kept here so tests can call crate::commands::*)
// =========================================================================

/// Returns `true` for paths that are pre-staged summary sidecars written by
/// `FtbModpackSource::stage_version_to_temp` (`.ftbpack.json`) or
/// `AtlauncherModpackSource::stage_version_to_temp` (`.atlpack.json`).
/// These files contain a serialised `ModpackSummary` and require no archive
/// parsing — they are deserialized directly by `read_staged_sidecar`.
fn is_staged_summary_sidecar(path: &str) -> bool {
    path.ends_with(".ftbpack.json") || path.ends_with(".atlpack.json")
}

/// Read a staged-summary sidecar (`.ftbpack.json` or `.atlpack.json`) back
/// into a `ModpackSummary`.  The format label in any error is derived from
/// the extension so FTB failures report `"ftb"` and ATLauncher failures
/// report `"atlauncher"`.
async fn read_staged_sidecar(path: &str) -> Result<ModpackSummary, crate::error::Error> {
    let format_label = if path.ends_with(".atlpack.json") {
        "atlauncher"
    } else {
        "ftb"
    };
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: path.to_owned(),
            details: e.to_string(),
        })?;
    serde_json::from_slice(&bytes).map_err(|e| crate::error::Error::ModpackManifestInvalid {
        format: format_label.into(),
        details: e.to_string(),
    })
}

/// Fetch a modpack project's version list from a Modrinth-shaped base.
/// Split out from the `modpack_get_versions` command so tests can
/// inject a wiremock base URL.
pub(crate) async fn fetch_modpack_versions(
    base: &str,
    project_id: &str,
) -> crate::error::Result<Vec<crate::mods::modpack::schema::ModpackVersionEntry>> {
    let url = format!("{base}/v2/project/{project_id}/version");
    let resp = crate::network::request::get(&url, &[], "modpacks").await?;
    if resp.status == 404 {
        return Err(crate::error::Error::ModsNotFound {
            platform: "modrinth".into(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(crate::error::Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    serde_json::from_slice(&resp.body).map_err(|e| crate::error::Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })
}

/// Minimal serde shape for the Modrinth `/v2/project/{id}` fields the
/// modpack detail modal consumes. Split out so tests inject a base URL.
#[derive(serde::Deserialize)]
struct MrModpackProject {
    body: String,
    source_url: Option<String>,
    wiki_url: Option<String>,
    #[serde(default)]
    gallery: Vec<MrGalleryEntry>,
}

#[derive(serde::Deserialize)]
struct MrGalleryEntry {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    featured: bool,
    #[serde(default)]
    ordering: Option<i64>,
}

pub(crate) async fn fetch_modrinth_modpack_project(
    base: &str,
    project_id: &str,
) -> crate::error::Result<crate::mods::modpack::schema::ModpackProject> {
    let url = format!("{base}/v2/project/{project_id}");
    let resp = crate::network::request::get(&url, &[], "modpacks").await?;
    if resp.status == 404 {
        return Err(crate::error::Error::ModsNotFound {
            platform: "modrinth".into(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(crate::error::Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    let p: MrModpackProject =
        serde_json::from_slice(&resp.body).map_err(|e| crate::error::Error::ModsDecode {
            platform: "modrinth".into(),
            details: e.to_string(),
        })?;
    let mut entries = p.gallery;
    entries.sort_by(|a, b| {
        b.featured.cmp(&a.featured).then(
            a.ordering
                .unwrap_or(i64::MAX)
                .cmp(&b.ordering.unwrap_or(i64::MAX)),
        )
    });
    let gallery = entries
        .into_iter()
        .filter(|e| crate::mods::render::is_safe_image_url(&e.url))
        .map(|e| crate::mods::platform::GalleryImage {
            url: e.url,
            title: e.title,
        })
        .collect();
    Ok(crate::mods::modpack::schema::ModpackProject {
        body_html: crate::mods::render::markdown_to_safe_html(&p.body),
        gallery,
        website_url: p.source_url.or(p.wiki_url),
    })
}

/// Pick the most-recently-published version, or `None` if the list is
/// empty or its newest entry's opaque Modrinth `id` already equals
/// `current_id`. Pure — split out so it is unit-testable.
pub(crate) fn latest_newer(
    mut versions: Vec<crate::mods::modpack::schema::ModpackVersionEntry>,
    current_id: &str,
) -> Option<crate::mods::modpack::schema::ModpackVersionEntry> {
    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    let latest = versions.into_iter().next()?;
    if latest.id == current_id {
        None
    } else {
        Some(latest)
    }
}

// =========================================================================
// Modpack-update helpers (used by modpack.rs domain file)
// =========================================================================

/// Best-effort recursive byte total; returns 0 on any error or missing dir.
fn dir_size_bytes(dir: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += dir_size_bytes(&entry.path());
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

/// Remove one pack-origin file from an instance: a `mods/` jar via the
/// mod registry, anything else by deleting the file at `install_path`.
/// Tries both the enabled and `.disabled` on-disk names — a user-disabled
/// old version must not orphan into a ghost mod (the registry record is
/// gone, so a leftover `.disabled` file would be re-adopted by the next
/// reconcile as a brand-new manual mod).
async fn remove_pack_file(
    inst_root: &std::path::Path,
    f: &crate::mods::installed::PackOriginFile,
) -> crate::error::Result<()> {
    if f.install_path.starts_with("mods/") {
        crate::mods::installed::remove(inst_root, &f.sha1).await?;
        let jar = crate::mods::installed::mods_dir(inst_root).join(&f.filename);
        if tokio::fs::try_exists(&jar).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&jar).await;
        }
        let disabled =
            crate::mods::installed::mods_dir(inst_root).join(format!("{}.disabled", f.filename));
        if tokio::fs::try_exists(&disabled).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&disabled).await;
        }
    } else {
        let p = inst_root.join(".minecraft").join(&f.install_path);
        if tokio::fs::try_exists(&p).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&p).await;
        }
    }
    Ok(())
}

// =========================================================================
// Tests (verbatim from original commands.rs)
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    /// A user-disabled pack mod lives on disk as `<name>.jar.disabled`;
    /// removing its pack entry must delete that variant too, or it orphans
    /// into a ghost manual mod at the next registry reconcile.
    #[tokio::test]
    async fn remove_pack_file_removes_disabled_variant() {
        let td = tempfile::TempDir::new().unwrap();
        let mods_dir = crate::mods::installed::mods_dir(td.path());
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        tokio::fs::write(mods_dir.join("x.jar.disabled"), b"bytes")
            .await
            .unwrap();
        let f = crate::mods::installed::PackOriginFile {
            sha1: "abc".into(),
            name: "x".into(),
            filename: "x.jar".into(),
            install_path: "mods/x.jar".into(),
            url: "https://cdn.modrinth.com/x.jar".into(),
            size: 5.0,
            project_id: String::new(),
            version_id: String::new(),
            env_client: crate::mods::modpack::schema::EnvSupport::Required,
            source: crate::mods::platform::ModSource::Modrinth,
        };
        remove_pack_file(td.path(), &f).await.unwrap();
        assert!(
            !tokio::fs::try_exists(mods_dir.join("x.jar.disabled"))
                .await
                .unwrap(),
            "the .disabled variant must be removed too"
        );
    }

    #[test]
    fn greet_includes_name() {
        let g = greet("World".to_string());
        assert!(g.message.contains("World"));
        assert!(g.message.contains("Lucerna"));
    }

    #[test]
    fn validate_accepts_normal_name() {
        assert!(validate_instance_name("My Pack").is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(matches!(
            validate_instance_name(""),
            Err(Error::InstanceNameEmpty)
        ));
    }

    #[test]
    fn validate_rejects_whitespace_only() {
        assert!(matches!(
            validate_instance_name("   \t  "),
            Err(Error::InstanceNameEmpty)
        ));
    }

    #[test]
    fn validate_trims_leading_trailing_whitespace_for_length_check() {
        // 32 chars surrounded by spaces — trimmed length is 32, valid.
        let name = format!("  {}  ", "a".repeat(32));
        assert!(validate_instance_name(&name).is_ok());
    }

    #[test]
    fn validate_accepts_exactly_32_chars() {
        assert!(validate_instance_name(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn validate_rejects_33_chars() {
        let result = validate_instance_name(&"a".repeat(33));
        assert!(matches!(
            result,
            Err(Error::InstanceNameTooLong {
                max: 32,
                actual: 33
            })
        ));
    }

    #[test]
    fn validate_counts_unicode_scalar_values_not_bytes() {
        // 30 cyrillic chars = 60 bytes in UTF-8 but 30 scalars — valid.
        assert!(validate_instance_name(&"я".repeat(30)).is_ok());
        // 33 cyrillic chars = 66 bytes — should still reject as 33 too long.
        let result = validate_instance_name(&"я".repeat(33));
        assert!(matches!(
            result,
            Err(Error::InstanceNameTooLong {
                max: 32,
                actual: 33
            })
        ));
    }

    // These tests verify the validate_instance_name call site — they do
    // NOT exercise the full Tauri command path (no AppHandle available
    // in unit tests). For full integration use the matrix harness.

    #[test]
    fn validate_rejects_at_create_call_site_path() {
        // The shape we want: anyone calling validate_instance_name
        // before reaching crate::instances::create_instance gets the
        // correct typed error. The function's a private guard, so this
        // is a behavioural assertion via the public helper.
        let r = validate_instance_name("");
        assert!(matches!(r, Err(Error::InstanceNameEmpty)));
        let r = validate_instance_name(&"x".repeat(33));
        assert!(matches!(r, Err(Error::InstanceNameTooLong { .. })));
    }

    #[tokio::test]
    async fn modpack_get_versions_parses_modrinth_list() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/abc/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{"id":"v1","name":"Pack 1.0","version_number":"1.0",
                     "game_versions":["1.20.1"],"loaders":["fabric"],
                     "date_published":"2026-05-01T00:00:00Z"}]"#,
            ))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let entries = crate::commands::fetch_modpack_versions(&server.uri(), "abc")
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "v1");
        assert_eq!(entries[0].game_versions, vec!["1.20.1"]);
    }

    #[tokio::test]
    async fn modpack_get_versions_non_2xx_is_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/missing/version"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = crate::commands::fetch_modpack_versions(&server.uri(), "missing")
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::error::Error::ModsNotFound { .. }),
            "got: {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_modrinth_modpack_project_renders_body_and_gallery() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"{"body":"# Pack\n\ntext","source_url":"https://src.example","wiki_url":null,
                    "gallery":[{"url":"https://media.modrinth.com/g.png","title":"G","featured":true,"ordering":1}]}"##,
            ))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let p = crate::commands::fetch_modrinth_modpack_project(&server.uri(), "abc")
            .await
            .unwrap();
        assert!(p.body_html.contains("<h1>"));
        assert_eq!(p.gallery[0].url, "https://media.modrinth.com/g.png");
        assert_eq!(p.website_url.as_deref(), Some("https://src.example"));
    }

    fn ver(num: &str, date: &str) -> crate::mods::modpack::schema::ModpackVersionEntry {
        crate::mods::modpack::schema::ModpackVersionEntry {
            id: format!("id-{num}"),
            name: num.into(),
            version_number: num.into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            date_published: date.into(),
        }
    }

    #[test]
    fn latest_newer_picks_newest_when_different() {
        let list = vec![
            ver("1.0", "2026-01-01T00:00:00Z"),
            ver("1.2", "2026-03-01T00:00:00Z"),
            ver("1.1", "2026-02-01T00:00:00Z"),
        ];
        // current id is "id-1.0"; newest by date is "1.2" → id "id-1.2"
        let r = crate::commands::latest_newer(list, "id-1.0");
        assert_eq!(r.map(|v| v.id), Some("id-1.2".to_string()));
    }

    #[test]
    fn latest_newer_none_when_already_latest() {
        let list = vec![
            ver("1.2", "2026-03-01T00:00:00Z"),
            ver("1.0", "2026-01-01T00:00:00Z"),
        ];
        // current id IS the newest → no update
        assert!(crate::commands::latest_newer(list, "id-1.2").is_none());
    }

    #[test]
    fn latest_newer_none_for_empty_list() {
        assert!(crate::commands::latest_newer(vec![], "id-1.0").is_none());
    }

    // --- staged-sidecar detection ---

    #[test]
    fn is_staged_summary_sidecar_accepts_ftbpack() {
        assert!(is_staged_summary_sidecar("/tmp/abc123.ftbpack.json"));
    }

    #[test]
    fn is_staged_summary_sidecar_accepts_atlpack() {
        // ATLauncher sidecar must also be recognised.
        assert!(is_staged_summary_sidecar("/tmp/abc123.atlpack.json"));
    }

    #[test]
    fn is_staged_summary_sidecar_rejects_mrpack() {
        assert!(!is_staged_summary_sidecar("/tmp/pack.mrpack"));
    }

    #[test]
    fn is_staged_summary_sidecar_rejects_zip() {
        assert!(!is_staged_summary_sidecar("/tmp/pack.zip"));
    }

    #[test]
    fn is_staged_summary_sidecar_rejects_plain_json() {
        assert!(!is_staged_summary_sidecar("/tmp/pack.json"));
    }
}

#[cfg(test)]
mod verify_cmd_tests {
    #[test]
    fn busy_error_has_stable_shape() {
        let e = crate::error::Error::InstanceBusy;
        let msg = format!("{e}");
        assert!(!msg.is_empty());
    }
}
