//! Clone an existing instance into a new one (same MC version + loader).
//!
//! Reuses the launcher-import pipeline's copy primitives ([`copy_category`],
//! [`ContentCategory`]) for the `.minecraft` content. Mods — together with the
//! installed-mods registry — and the custom icon always travel with the clone,
//! so the copy is immediately manageable (update checks, enable/disable,
//! dependency graph) without a re-enrich pass; everything else is opt-in via
//! [`CloneOptions`]. The source instance is opened read-only.
//!
//! Unlike the launcher import (best-effort categories), every copy failure
//! here is fatal and rolls the clone back: a clone is a cheap local operation
//! the user can retry instantly, and a silently partial copy (half the saves)
//! is worse than an honest error.

use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{Error, Result};
use crate::instances::import::model::ContentCategory;
use crate::instances::import::pipeline::copy_category;
use crate::instances::schema::{InstanceFile, InstanceWithStatus};

/// What to copy beyond the always-copied set (mods + registry + icon).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct CloneOptions {
    pub saves: bool,
    /// Instance settings: `max_heap_mb`, `min_heap_mb`, `extra_jvm_args`.
    pub settings: bool,
    /// `resourcepacks/` + `shaderpacks/` under one checkbox.
    pub packs: bool,
    pub config: bool,
    pub options_txt: bool,
    pub playtime: bool,
}

/// Per-category copy progress streamed to the UI (same counts shape the
/// launcher import emits).
#[derive(Debug, Clone, Serialize, Type)]
pub struct CloneProgress {
    pub category: ContentCategory,
    pub current: u32,
    pub total: u32,
}

/// Build the clone's `instance.json` record from the source's.
///
/// Always carried: MC version, loader (+version), all `mrpack_*` provenance (a
/// clone of a pack instance IS that pack — the update flow keeps working) and
/// `imported_from` (content provenance stays true for the copy). Settings are
/// gated on `copy_settings`; when off the clone gets the same defaults as a
/// freshly created instance. Never carried: `integrity` (the clone has not
/// been verified), `handled_log_sig` (no logs are copied) and
/// `created_from_server` (the origin server keeps exactly one canonical
/// client instance).
pub fn cloned_instance_file(
    src: &InstanceFile,
    id: String,
    name: String,
    now_ms: f64,
    copy_settings: bool,
    default_heap_mb: u32,
) -> InstanceFile {
    InstanceFile {
        id,
        name,
        mc_version: src.mc_version.clone(),
        loader: src.loader,
        loader_version: src.loader_version.clone(),
        max_heap_mb: if copy_settings {
            src.max_heap_mb
        } else {
            default_heap_mb
        },
        min_heap_mb: if copy_settings { src.min_heap_mb } else { None },
        extra_jvm_args: if copy_settings {
            src.extra_jvm_args.clone()
        } else {
            String::new()
        },
        created_unix_ms: now_ms,
        mrpack_name: src.mrpack_name.clone(),
        mrpack_version: src.mrpack_version.clone(),
        mrpack_project_id: src.mrpack_project_id.clone(),
        mrpack_source: src.mrpack_source,
        mrpack_summary: src.mrpack_summary.clone(),
        mrpack_version_id: src.mrpack_version_id.clone(),
        integrity: None,
        imported_from: src.imported_from.clone(),
        created_from_server: None,
        handled_log_sig: None,
    }
}

/// Copy the source instance's content into the freshly-reserved clone dir.
/// Missing *source* dirs/files are fine (nothing to copy); any actual copy
/// failure is fatal — the caller rolls the whole clone back.
pub fn copy_clone_content(
    src_root: &Path,
    dst_root: &Path,
    options: &CloneOptions,
    progress: &mut dyn FnMut(ContentCategory, u32, u32),
) -> Result<()> {
    let src_mc = src_root.join(".minecraft");
    let dst_mc = dst_root.join(".minecraft");

    // Mods always travel with the clone, along with the installed-mods
    // registry, so the copy stays manageable without a rehash/re-enrich pass
    // (the registry reconciles against `mods/` on every read by design).
    copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |cur, tot| {
        progress(ContentCategory::Mods, cur, tot)
    })?;
    copy_meta_file(
        &crate::mods::installed::registry_path(src_root),
        &crate::mods::installed::registry_path(dst_root),
    )?;

    let mut selected: Vec<ContentCategory> = Vec::new();
    if options.saves {
        selected.push(ContentCategory::Saves);
    }
    if options.config {
        selected.push(ContentCategory::Config);
    }
    if options.packs {
        selected.push(ContentCategory::ResourcePacks);
        selected.push(ContentCategory::Shaderpacks);
    }
    if options.options_txt {
        selected.push(ContentCategory::OptionsTxt);
    }
    for cat in selected {
        copy_category(&src_mc, &dst_mc, cat, &mut |cur, tot| {
            progress(cat, cur, tot)
        })?;
    }

    // The custom picture always travels with the clone.
    copy_meta_file(&src_root.join("icon.png"), &dst_root.join("icon.png"))?;

    if options.playtime {
        copy_meta_file(
            &crate::playtime::playtime_path(src_root),
            &crate::playtime::playtime_path(dst_root),
        )?;
    }
    Ok(())
}

/// Copy a single optional metadata file. A missing source is fine; a failed
/// copy is fatal (the caller rolls back the whole clone).
fn copy_meta_file(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_file() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    std::fs::copy(src, dst).map_err(|e| Error::io(dst.display().to_string(), e))?;
    Ok(())
}

/// Full clone orchestration: reserve a unique slug dir for `new_name`, copy
/// the content per `options`, write `instance.json` last, and return the new
/// instance with computed `ready`/`has_icon`. Any failure removes the
/// half-built directory (the reserved slug never leaks).
///
/// The clone does NOT become the active instance — the user is branching,
/// not switching.
pub fn clone_instance(
    app: &tauri::AppHandle,
    source_id: &str,
    new_name: String,
    options: &CloneOptions,
    mut progress: impl FnMut(ContentCategory, u32, u32),
) -> Result<InstanceWithStatus> {
    let src = crate::instances::read_instance(app, source_id)?;
    let src_root =
        crate::paths::instance_dir(app, source_id).map_err(|e| Error::io("<instance_dir>", e))?;
    let instances_parent =
        crate::paths::instances_dir(app).map_err(|e| Error::io("<instances_dir>", e))?;

    let (id, dst_root) =
        crate::naming::reserve_unique_dir(&instances_parent, &new_name, "instance")?;
    let cleanup = crate::naming::DirCleanup::new(&dst_root);

    std::fs::create_dir_all(dst_root.join(".minecraft"))
        .map_err(|e| Error::io(dst_root.display().to_string(), e))?;
    copy_clone_content(&src_root, &dst_root, options, &mut progress)?;

    let file = cloned_instance_file(
        &src,
        id.clone(),
        new_name,
        crate::instances::unix_ms_f64(),
        options.settings,
        crate::instances::memory::default_heap_mb(crate::platform::total_system_ram_mb()),
    );
    let json_path =
        crate::paths::instance_json(app, &id).map_err(|e| Error::io("<instance_json>", e))?;
    crate::instances::store::write_instance_json(&json_path, &file)?;

    let versions_dir =
        crate::paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let ready = crate::instances::status::ready_status(&versions_dir, &file);
    let has_icon = crate::instances::icon::has_icon(&dst_root.join("icon.png"));
    cleanup.keep();
    Ok(InstanceWithStatus::from_file(&file, ready, has_icon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::{ImportProvenance, LoaderKind};

    fn all_off() -> CloneOptions {
        CloneOptions {
            saves: false,
            settings: false,
            packs: false,
            config: false,
            options_txt: false,
            playtime: false,
        }
    }

    fn source_file() -> InstanceFile {
        InstanceFile {
            id: "Source".into(),
            name: "Source".into(),
            mc_version: "1.20.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 8192,
            min_heap_mb: Some(4096),
            extra_jvm_args: "-XX:+UseG1GC".into(),
            created_unix_ms: 1_700_000_000_000.0,
            mrpack_name: Some("Fabulously Optimized".into()),
            mrpack_version: Some("5.9.0".into()),
            mrpack_project_id: Some("1KVo5zza".into()),
            mrpack_source: Some(crate::mods::platform::ModSource::Modrinth),
            mrpack_summary: Some("A pack".into()),
            mrpack_version_id: Some("vyRB9jtS".into()),
            integrity: Some(crate::verify::IntegrityStatus {
                healthy: false,
                checked_unix_ms: 1_700_000_000_000.0,
                categories: vec![],
                problem_count: 2,
            }),
            imported_from: Some(ImportProvenance {
                launcher: crate::instances::schema::ForeignLauncher::Prism,
                source_name: "FO".into(),
                source_path: r"C:\prism\instances\FO".into(),
                imported_unix_ms: 1_700_000_000_000.0,
            }),
            created_from_server: Some("srv-1".into()),
            handled_log_sig: Some("sig".into()),
        }
    }

    // ---- cloned_instance_file -------------------------------------------

    #[test]
    fn cloned_file_copies_settings_when_selected() {
        let c = cloned_instance_file(
            &source_file(),
            "Copy".into(),
            "Copy".into(),
            1.0,
            true,
            3000,
        );
        assert_eq!(c.max_heap_mb, 8192);
        assert_eq!(c.min_heap_mb, Some(4096));
        assert_eq!(c.extra_jvm_args, "-XX:+UseG1GC");
    }

    #[test]
    fn cloned_file_uses_defaults_when_settings_not_selected() {
        let c = cloned_instance_file(
            &source_file(),
            "Copy".into(),
            "Copy".into(),
            1.0,
            false,
            3000,
        );
        assert_eq!(c.max_heap_mb, 3000, "adaptive default, like a fresh create");
        assert_eq!(c.min_heap_mb, None);
        assert_eq!(c.extra_jvm_args, "");
    }

    #[test]
    fn cloned_file_always_carries_version_loader_and_mrpack() {
        let src = source_file();
        let c = cloned_instance_file(&src, "Copy".into(), "Copy 2".into(), 42.0, false, 3000);
        assert_eq!(c.mc_version, src.mc_version);
        assert_eq!(c.loader, src.loader);
        assert_eq!(c.loader_version, src.loader_version);
        assert_eq!(c.mrpack_name, src.mrpack_name);
        assert_eq!(c.mrpack_version, src.mrpack_version);
        assert_eq!(c.mrpack_project_id, src.mrpack_project_id);
        assert_eq!(c.mrpack_source, src.mrpack_source);
        assert_eq!(c.mrpack_summary, src.mrpack_summary);
        assert_eq!(c.mrpack_version_id, src.mrpack_version_id);
        assert_eq!(c.imported_from, src.imported_from);
        // Fresh identity fields.
        assert_eq!(c.id, "Copy");
        assert_eq!(c.name, "Copy 2");
        assert_eq!(c.created_unix_ms, 42.0);
    }

    #[test]
    fn cloned_file_clears_integrity_handled_sig_and_server_link() {
        let c = cloned_instance_file(
            &source_file(),
            "Copy".into(),
            "Copy".into(),
            1.0,
            true,
            3000,
        );
        assert_eq!(c.integrity, None, "clone has not been verified");
        assert_eq!(c.handled_log_sig, None, "no logs are copied");
        assert_eq!(
            c.created_from_server, None,
            "origin server keeps one canonical client instance"
        );
    }

    // ---- copy_clone_content ---------------------------------------------

    fn write(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// Source instance dir with one file in every copyable location.
    fn full_source(root: &Path) {
        write(&root.join(".minecraft/mods/sodium.jar"), "MOD");
        write(&root.join("lucerna/installed-mods.json"), "{\"mods\":[]}");
        write(&root.join(".minecraft/saves/world/level.dat"), "SAVE");
        write(&root.join(".minecraft/config/sodium.toml"), "CFG");
        write(&root.join(".minecraft/resourcepacks/pack.zip"), "RP");
        write(&root.join(".minecraft/shaderpacks/shader.zip"), "SH");
        write(&root.join(".minecraft/options.txt"), "version:3465");
        write(&root.join("icon.png"), "PNG");
        write(
            &root.join(".lucerna/playtime.json"),
            "{\"total_seconds\":60}",
        );
        write(
            &root.join(".lucerna/journal.jsonl"),
            "{\"at_unix_ms\":1.0,\"event\":{\"kind\":\"content\",\"action\":\"mod_installed\",\"subject\":\"Sodium\",\"from_version\":null,\"to_version\":null,\"affected\":null}}\n",
        );
    }

    fn no_progress() -> impl FnMut(ContentCategory, u32, u32) {
        |_, _, _| {}
    }

    #[test]
    fn copy_content_always_copies_mods_and_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);

        copy_clone_content(&src, &dst, &all_off(), &mut no_progress()).unwrap();

        assert!(dst.join(".minecraft/mods/sodium.jar").is_file());
        assert!(dst.join("lucerna/installed-mods.json").is_file());
        assert!(!dst.join(".minecraft/saves").exists(), "saves not selected");
        assert!(!dst.join(".minecraft/config").exists());
        assert!(!dst.join(".minecraft/options.txt").exists());
        assert!(!dst.join(".lucerna/playtime.json").exists());
    }

    #[test]
    fn copy_content_copies_saves_only_when_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);
        let options = CloneOptions {
            saves: true,
            ..all_off()
        };

        copy_clone_content(&src, &dst, &options, &mut no_progress()).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.join(".minecraft/saves/world/level.dat")).unwrap(),
            "SAVE"
        );
        assert!(!dst.join(".minecraft/config").exists());
    }

    #[test]
    fn copy_content_packs_copies_resourcepacks_and_shaderpacks() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);
        let options = CloneOptions {
            packs: true,
            ..all_off()
        };

        copy_clone_content(&src, &dst, &options, &mut no_progress()).unwrap();

        assert!(dst.join(".minecraft/resourcepacks/pack.zip").is_file());
        assert!(dst.join(".minecraft/shaderpacks/shader.zip").is_file());
        assert!(!dst.join(".minecraft/saves").exists());
    }

    #[test]
    fn copy_content_copies_options_txt_icon_and_playtime() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);
        let options = CloneOptions {
            options_txt: true,
            playtime: true,
            ..all_off()
        };

        copy_clone_content(&src, &dst, &options, &mut no_progress()).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.join(".minecraft/options.txt")).unwrap(),
            "version:3465"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("icon.png")).unwrap(),
            "PNG"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join(".lucerna/playtime.json")).unwrap(),
            "{\"total_seconds\":60}"
        );
    }

    #[test]
    fn clone_never_inherits_the_source_journal() {
        // The journal is the history of what was done to THIS instance, so a
        // clone starts empty — nothing has been done to it yet. `.lucerna/` is
        // copied file-by-file (playtime only); this pins that a future blanket
        // copy of the whole meta dir cannot silently hand over the history.
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);
        let everything = CloneOptions {
            saves: true,
            settings: true,
            packs: true,
            config: true,
            options_txt: true,
            playtime: true,
        };

        copy_clone_content(&src, &dst, &everything, &mut no_progress()).unwrap();

        assert!(
            dst.join(".lucerna/playtime.json").is_file(),
            "playtime still copies when selected"
        );
        assert!(
            !dst.join(".lucerna/journal.jsonl").exists(),
            "the clone must not inherit the source instance's journal"
        );
    }

    #[test]
    fn copy_content_skips_playtime_when_not_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);

        copy_clone_content(&src, &dst, &all_off(), &mut no_progress()).unwrap();

        assert!(!dst.join(".lucerna").exists());
        assert!(dst.join("icon.png").is_file(), "icon copies regardless");
    }

    #[test]
    fn copy_content_tolerates_missing_source_dirs() {
        // A bare instance: `.minecraft` exists but is empty — no mods dir, no
        // registry, no icon, no playtime. Everything selected must still be Ok.
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        std::fs::create_dir_all(src.join(".minecraft")).unwrap();
        let options = CloneOptions {
            saves: true,
            settings: true,
            packs: true,
            config: true,
            options_txt: true,
            playtime: true,
        };

        copy_clone_content(&src, &dst, &options, &mut no_progress()).unwrap();

        assert!(!dst.join(".minecraft/mods").exists());
        assert!(!dst.join("icon.png").exists());
    }

    #[test]
    fn copy_content_leaves_source_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);
        let options = CloneOptions {
            saves: true,
            settings: true,
            packs: true,
            config: true,
            options_txt: true,
            playtime: true,
        };

        copy_clone_content(&src, &dst, &options, &mut no_progress()).unwrap();

        assert_eq!(
            std::fs::read_to_string(src.join(".minecraft/mods/sodium.jar")).unwrap(),
            "MOD"
        );
        assert_eq!(
            std::fs::read_to_string(src.join(".minecraft/saves/world/level.dat")).unwrap(),
            "SAVE"
        );
        assert_eq!(
            std::fs::read_to_string(src.join("icon.png")).unwrap(),
            "PNG"
        );
    }

    #[test]
    fn copy_content_reports_progress_per_category() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dst) = (tmp.path().join("src"), tmp.path().join("dst"));
        full_source(&src);
        let options = CloneOptions {
            saves: true,
            ..all_off()
        };

        let mut seen: Vec<(ContentCategory, u32, u32)> = Vec::new();
        copy_clone_content(&src, &dst, &options, &mut |cat, cur, tot| {
            seen.push((cat, cur, tot));
        })
        .unwrap();

        assert!(seen.contains(&(ContentCategory::Mods, 1, 1)));
        assert!(seen.contains(&(ContentCategory::Saves, 1, 1)));
    }
}
