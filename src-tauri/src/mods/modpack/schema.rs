//! Normalised types shared between Modrinth and CurseForge modpack
//! parsers and the import orchestrator. Every public type derives
//! `specta::Type` and crosses the IPC boundary as-is.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::installed::{PackOrigin, PackOriginFile};
use crate::mods::platform::{LoaderKind, ModSource};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModpackFormat {
    Modrinth,
    Curseforge,
    /// Feed The Beast — API-only source; no local archive format.
    Ftb,
    /// ATLauncher — API + CDN source; no local archive format.
    Atlauncher,
}

/// Sort order for `/v2/search`. Maps to Modrinth's `index=<value>` query
/// param. `follows` is omitted — not useful for modpack discovery; can
/// be added later if requested.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModpackSort {
    Relevance,
    Downloads,
    Newest,
    Updated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvSupport {
    Required,
    Optional,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvableReason {
    DistributionDisabled,
    HostNotAllowed,
    UnsafePath,
    /// A file whose integrity cannot be verified because no SHA-1 checksum was
    /// available from any source (the pack manifest and any secondary API all
    /// returned an absent or empty hash). Installing such a file would be TOFU
    /// (trust-on-first-use); it is surfaced as manually unresolvable instead.
    MissingChecksum,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModpackFile {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub filename: String,
    pub install_path: String,
    pub sha1: String,
    /// ATLauncher server/direct mods ship an md5 (not sha1). When set, the
    /// installer verifies md5 on download and records the *computed* sha1 as
    /// the file's identity. `None` for all other sources. `#[serde(default)]`
    /// keeps pre-existing sidecars/registries loadable.
    #[serde(default)]
    pub md5: Option<String>,
    pub url: String,
    pub size: f64,
    pub env_client: EnvSupport,
    pub source: ModSource,
}

/// A file inside the pack's `overrides/` that the importer chose NOT to
/// extract because it exceeded the per-file size cap. In practice this is
/// an inert, non-loadable blob — most often a `.rar`/`.7z`/`.zip` an author
/// left in `overrides/mods/`, which Minecraft never loads (it scans
/// `mods/*.jar` only). The import still succeeds; this is surfaced purely
/// so the user is told what was deliberately left out (transparency).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct SkippedOverride {
    /// The override-relative path of the skipped entry, e.g. `mods/mods.rar`.
    pub path: String,
    /// The entry's declared uncompressed size in bytes. f64 not u64 —
    /// specta forbids BigInt-style exports.
    pub size: f64,
}

/// A jar installed into an instance whose loader family the instance cannot
/// load — e.g. a Fabric-only jar in a Forge instance. Forge never reads
/// `fabric.mod.json`, so the jar is inert (loads nothing) and is safe to ignore
/// or remove. The import still succeeds; surfaced purely for transparency.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct InertLoaderJar {
    pub filename: String,
    /// Loader family detected in the jar's descriptor — "Fabric" / "Forge".
    pub detected_loader: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ModpackUnresolvable {
    pub reason: UnresolvableReason,
    /// Best-effort human label: the CurseForge displayName, or the
    /// Modrinth file path for host/path rejections.
    pub mod_name: String,
    /// CurseForge project page, or a direct download URL — empty when
    /// no manual route exists (e.g. an unsafe path).
    pub manual_action_url: String,
    /// Expected jar filename. The CF/Modrinth APIs provide it even when
    /// the file itself cannot be downloaded — used to auto-detect a
    /// later manual install.
    pub filename: String,
    /// Expected file size in bytes. f64 not u64 — specta forbids
    /// BigInt-style exports.
    pub size: f64,
    /// Expected sha1, lowercased, when the platform supplied one. A
    /// CurseForge distribution-disabled file may carry only an md5 —
    /// `None` in that case.
    pub sha1: Option<String>,
    /// Platform project id of the missing mod, when known — used to
    /// detect a *different version* of the same mod installed via the
    /// launcher's mod browser (which records project_id in the
    /// registry). CurseForge: the mod id, always available at parse
    /// time. Modrinth `HostNotAllowed`: not derivable from a non-CDN
    /// URL — `None`. `#[serde(default)]` so pre-C registry files load.
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModpackSummary {
    pub format: ModpackFormat,
    pub name: String,
    pub version: String,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub files: Vec<ModpackFile>,
    pub unresolvable: Vec<ModpackUnresolvable>,
    pub has_overrides: bool,
    pub has_client_overrides: bool,
    pub has_saves_in_overrides: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: f64,
    pub latest_mc_version: Option<String>,
    pub supported_loaders: Vec<LoaderKind>,
    /// Which platform this hit came from. The version drawer + import
    /// flow branch on it.
    pub source: ModSource,
    /// CurseForge `allowModDistribution`. `None` for Modrinth hits and
    /// for CurseForge packs whose flag is `null` (unknown);
    /// `Some(false)` = the author disabled third-party launcher
    /// downloads — the UI surfaces an "Open on CurseForge" fallback.
    pub distribution_allowed: Option<bool>,
    /// Display name of the pack author/owner, when the source's listing
    /// exposes one. `None` for sources that omit it (ATLauncher's public
    /// catalogue) — the card then shows no author line. Modrinth: search
    /// `author` (project owner). FTB: first entry of the pack's `authors`.
    pub author: Option<String>,
}

/// Full detail of a modpack project for the detail modal's Overview tab.
/// Header fields (title, author, icon, downloads, distribution flag) stay
/// on the `ModpackHit` the modal already holds; this carries only what
/// the Overview tab adds.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackProject {
    /// Sanitized HTML of the pack's long description. Empty when none.
    pub body_html: String,
    pub gallery: Vec<crate::mods::platform::GalleryImage>,
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackSearchPage {
    pub hits: Vec<ModpackHit>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

/// Coarse-grained phase emitted on the `modpack_progress` channel.
/// Per-mod download/verify/copy ticks continue on the existing
/// `install_progress` channel from sub-3 so PhaseStatusRow + the
/// import progress view can both render fine-grained state.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ModpackProgress {
    Inspecting,
    CreatingInstance {
        name: String,
    },
    InstallingFile {
        current: u32,
        total: u32,
        file_name: String,
    },
    ExtractingOverrides {
        current: u32,
        total: u32,
    },
    Enriching,
    /// Terminal phase marker — deliberately **payload-free**.
    ///
    /// It used to carry the import's result (skipped overrides, inert jars,
    /// per-file report rows). That was a real bug, not a style question: a
    /// channel message is delivered out-of-band from the command's own
    /// response, and Tauri routes any JSON body at or above
    /// `MAX_JSON_DIRECT_EXECUTE_THRESHOLD` (8192 bytes, `ipc/channel.rs`)
    /// through a *second* async IPC round trip that lands after the response
    /// has already resolved. A 37-file pack serialises to ~10 KB, so the UI
    /// read its captured local while it was still empty and every install
    /// report came back "0 files".
    ///
    /// The rule this encodes: a task's terminal result travels on the
    /// command's **return value** (`ModpackImportOutcome`,
    /// `ModpackUpdateOutcome`); the channel carries progress only. Adding a
    /// field back here breaks `done_is_a_payload_free_phase_marker`.
    Done,
}

/// What `modpack_import` returns: the created instance plus everything the
/// completion toast and the install report need.
///
/// These ride the return value rather than `ModpackProgress::Done` — see that
/// variant's doc comment for the transport bug that forced the split.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackImportOutcome {
    pub instance: crate::instances::schema::InstanceWithStatus,
    /// Overrides skipped during extraction because they exceeded the per-file
    /// size cap (non-loadable blobs — see `SkippedOverride`). Empty in the
    /// common case; non-empty drives a non-fatal "N file(s) skipped" note on
    /// the import-complete toast.
    pub skipped_overrides: Vec<SkippedOverride>,
    /// Installed jars built for a loader family this instance cannot load
    /// (inert — e.g. a Fabric jar on a Forge instance — see `InertLoaderJar`).
    /// Empty in the common case; non-empty drives a non-fatal "N inert jar(s)"
    /// note on the import-complete toast.
    pub inert_loader_jars: Vec<InertLoaderJar>,
    /// One row per file this import touched — what it was, where it came from,
    /// how big, and what happened to it. The UI hands these to the task
    /// registry; the backend separately persists them as an install report.
    pub details: Vec<crate::tasks::TaskDetail>,
}

/// What `modpack_apply_update` returns. Same reasoning as
/// [`ModpackImportOutcome`]; a version bump never touches `overrides/`, so
/// there is nothing to skip and no `skipped_overrides` field.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackUpdateOutcome {
    pub instance: crate::instances::schema::InstanceWithStatus,
    /// The mods dir is re-classified by an update, so a switch that changes
    /// the loader family reports which bundled jars have stopped loading.
    pub inert_loader_jars: Vec<InertLoaderJar>,
    /// Per-file rows for the removals + installs the update performed,
    /// including per-file failures (this path continues past them).
    pub details: Vec<crate::tasks::TaskDetail>,
}

/// Reconciled state of a `PackOrigin.missing_mods` entry against the
/// installed jars. Computed by `compute_status`.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissingModState {
    /// The exact file the pack pinned is installed.
    Installed,
    /// The mod is installed, but not the pinned file — a different version.
    DifferentVersion,
    /// No installed jar matches this mod by any signal.
    Missing,
    /// No pinned/different file is installed, but the user installed a
    /// substitute from another source (e.g. Modrinth) and we recorded the
    /// link in `PackOrigin.resolved_missing`. Treated as resolved, but
    /// surfaced distinctly so the UI does not claim the pinned file is present.
    Substituted,
}

/// One `PackOrigin.missing_mods` entry paired with its live
/// classification — `installed` (the pinned file), `different_version`
/// (the mod is present, but not the pinned file), `missing` (no matching
/// jar), or `substituted` (the user installed an alternative from another
/// source, recorded in `PackOrigin.resolved_missing`).
/// Computed by `compute_status`; consumed by the imported-pack drawer
/// and the Overview indicator.
#[derive(Debug, Clone, Serialize, Type)]
pub struct MissingModStatus {
    pub entry: ModpackUnresolvable,
    pub state: MissingModState,
}

/// One-shot diff of "what was the pack at import time" vs "what's
/// actually on disk now". The UI uses this to draw the modified-tag on
/// imported cards and the removed-from-pack section in the drawer.
/// Returned by `modpack_status` for pack-originated instances; the
/// command returns `None` for instances that have no `pack_origin`
/// recorded (= manually created or pre-bundle-2 imports).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackStatus {
    /// The frozen snapshot captured at import time.
    pub origin: PackOrigin,
    /// SHA-1 of every mod currently installed in the instance (the
    /// reconciled `mods_list_installed` set). Lowercased.
    pub installed_shas: Vec<String>,
    /// Pack files whose SHA-1 is no longer present in `installed_shas`
    /// (the user disabled+removed, uninstalled, or never installed).
    pub removed_files: Vec<PackOriginFile>,
    /// Number of installed mods whose SHA-1 is NOT in `origin.files`.
    /// These are user additions made after the import landed.
    pub added_count: u32,
    /// `!removed_files.is_empty() || added_count > 0`. Pre-computed
    /// here so the UI doesn't have to.
    pub is_modified: bool,
    /// Mods the import could not auto-download, each tagged with whether
    /// the user has since added it. Empty for instances imported before
    /// this feature and for non-pack instances.
    pub missing_mods: Vec<MissingModStatus>,
    /// What the pack's own completer mod is still waiting for, if it has one.
    /// Set by the command, not by `compute_status` — the helper reads the
    /// instance directory and `compute_status` is pure.
    ///
    /// `#[serde(default)]` so specta emits it OPTIONAL — without it every
    /// existing `ModpackStatus` literal in the frontend stops type-checking.
    #[serde(default)]
    pub pack_completion: Option<crate::mods::pack_completion::PackCompletion>,
}

/// A mod/asset present in both the installed pack and the new version
/// but at a different SHA-1 — i.e. the pack bumped this file's version.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackUpdateEntry {
    pub old: PackOriginFile,
    pub new: ModpackFile,
}

/// A Minecraft / loader version change between the installed pack and
/// the new version.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackVersionBump {
    pub old_game_version: String,
    pub new_game_version: String,
    pub old_loader_version: Option<String>,
    pub new_loader_version: Option<String>,
}

/// The result of diffing a new pack version against the installed
/// `pack_origin`. `added` / `updated` carry the new files to install;
/// `removed` carries the old files to delete. Bundled (`overrides/`,
/// empty-`url`) entries are excluded — a normal update never touches
/// `overrides/`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackUpdateDiff {
    pub added: Vec<ModpackFile>,
    pub removed: Vec<PackOriginFile>,
    pub updated: Vec<ModpackUpdateEntry>,
    pub version_bump: Option<ModpackVersionBump>,
    pub new_version_number: String,
}

/// One entry in a modpack project's version list, as shown in the
/// modpack version drawer. Mirrors the subset of the Modrinth
/// `/v2/project/<id>/version` response the UI consumes.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ModpackVersionEntry {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::LoaderKind;

    #[test]
    fn modpack_file_md5_defaults_to_none_for_legacy_json() {
        // A sidecar written before the md5 field existed must still deserialize.
        let legacy = serde_json::json!({
            "project_id": "1", "version_id": "abc", "name": "n", "filename": "n.jar",
            "install_path": "mods/n.jar", "sha1": "abc", "url": "https://x/y",
            "size": 1.0, "env_client": "required", "source": "ftb"
        });
        let f: ModpackFile = serde_json::from_value(legacy).unwrap();
        assert_eq!(f.md5, None);
    }

    #[test]
    fn modpack_summary_round_trips_json() {
        let s = ModpackSummary {
            format: ModpackFormat::Ftb,
            name: "P".into(),
            version: "1".into(),
            game_version: "1.20.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.1".into()),
            files: vec![],
            unresolvable: vec![],
            has_overrides: false,
            has_client_overrides: false,
            has_saves_in_overrides: false,
        };
        let j = serde_json::to_vec(&s).unwrap();
        let back: ModpackSummary = serde_json::from_slice(&j).unwrap();
        assert_eq!(back.format, ModpackFormat::Ftb);
        assert_eq!(back.name, "P");
    }

    #[test]
    fn done_is_a_payload_free_phase_marker() {
        // The invariant, not an incidental shape: anything the UI must READ
        // belongs on the command's return value, because a channel message
        // over 8192 bytes is delivered through a second async IPC round trip
        // that lands after the command's response has already resolved (see
        // `ModpackProgress::Done`'s doc comment). Adding a field here
        // re-opens that bug, so this test exists to go red when someone does.
        let j = serde_json::to_value(ModpackProgress::Done).unwrap();
        assert_eq!(j, serde_json::json!({ "phase": "done" }));
    }
}
