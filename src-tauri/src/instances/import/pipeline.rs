//! Launcher-import pipeline: copy selected content from a read-only
//! foreign instance into a fresh Lucerna instance, recover mod identities,
//! and finalize (with rollback on a mandatory-phase failure). The source
//! is opened read-only — nothing under it is ever written.

use std::path::Path;

use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::instances::import::model::{
    is_injected_mod, ContentCategory, ForeignInstance, ImportPlan, ImportProgress, KnownMod,
};
use crate::instances::schema::ImportProvenance;
use crate::mods::modpack::path_safety::is_safe_relative_path;
use crate::mods::platform::InstalledMod;

/// Progress callback for a copy: (files_done, files_total).
type CopyProgress<'a> = dyn FnMut(u32, u32) + 'a;

/// Copy one content category from `src_mc/<cat>` to `dst_mc/<cat>`,
/// recursively. The source is opened read-only — this never writes under
/// `src_mc`. Symlinked entries that would escape the category dir are
/// skipped. Returns the number of files copied.
pub fn copy_category(
    src_mc: &Path,
    dst_mc: &Path,
    cat: ContentCategory,
    progress: &mut CopyProgress<'_>,
) -> Result<u32> {
    let rel = cat.rel_path();
    let src = src_mc.join(rel);
    let dst = dst_mc.join(rel);

    if cat.is_file() {
        if !src.is_file() {
            return Ok(0);
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(&dst, e))?;
        }
        std::fs::copy(&src, &dst).map_err(|e| io(&dst, e))?;
        progress(1, 1);
        return Ok(1);
    }

    if !src.is_dir() {
        return Ok(0);
    }
    let files = collect_files(&src)?;
    let total = files.len() as u32;
    let mut done = 0u32;
    for relpath in &files {
        // Defense in depth: the relative path under the category must be safe.
        let rel_str = relpath.to_string_lossy().replace('\\', "/");
        if !is_safe_relative_path(&rel_str) {
            continue;
        }
        // Skip launcher-injected mods (e.g. TLauncher's tl_skin_cape_*) — they
        // are launcher cruft, not user content.
        if cat == ContentCategory::Mods {
            let fname = relpath
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if is_injected_mod(&fname) {
                continue;
            }
        }
        let from = src.join(relpath);
        let to = dst.join(relpath);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(&to, e))?;
        }
        std::fs::copy(&from, &to).map_err(|e| io(&to, e))?;
        done += 1;
        progress(done, total);
    }
    Ok(done)
}

/// Relative file paths under `root`, not following symlinked dirs.
///
/// `pub(crate)`: also reused by [`crate::instances::clone::copy_dir_recursive`]
/// for the datapack library, which needs the same "list every real file
/// under a tree" recursion but is not a `.minecraft`-relative
/// [`ContentCategory`] and so cannot go through [`copy_category`] itself.
pub(crate) fn collect_files(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).map_err(|e| io(&d, e))?.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let p = entry.path();
            if ft.is_symlink() {
                continue; // never follow links out of the tree
            } else if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_path_buf());
                }
            }
        }
    }
    Ok(out)
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::io(path.display().to_string(), e)
}

/// Turn copied `(filename, sha1)` pairs into registry records, applying a
/// `KnownMod` identity when the manifest provided one (matched by
/// filename). Jars without a known identity are left untracked
/// (`source: None`, `enrich_attempted: false`) so the later hash-enrich
/// pass can try to recover them.
pub fn build_installed_records(
    jars: &[(String, String)],
    known: &[KnownMod],
    installed_at: &str,
) -> Vec<InstalledMod> {
    jars.iter()
        .map(|(filename, sha1)| {
            let id = known.iter().find(|k| &k.filename == filename);
            InstalledMod {
                filename: filename.clone(),
                sha1: sha1.clone(),
                source: id.map(|k| k.source),
                project_id: id.map(|k| k.project_id.clone()),
                version_id: id.and_then(|k| k.version_id.clone()),
                name: filename.clone(),
                version_number: None,
                installed_at: installed_at.to_string(),
                enabled: true,
                enrich_attempted: false,
                requires: vec![],
            }
        })
        .collect()
}

/// Register freshly-copied loose jars in an instance's installed-mods
/// registry, then best-effort hash-enrich the untracked ones. Shared by the
/// launcher-import pipeline and the "client instance from server" flow.
/// `known` applies manifest identities by filename (pass `&[]` when there is
/// no manifest). Returns the untracked count after enrich. A registry write
/// failure is propagated; enrich failures degrade to "no match".
pub async fn adopt_copied_jars(
    instance_root: &Path,
    mods_dir: &Path,
    known: &[KnownMod],
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
) -> Result<u32> {
    let jars = hash_jars(mods_dir);
    let records = build_installed_records(&jars, known, &now_rfc3339());
    crate::mods::installed::register_imported_mods(instance_root, records).await?;
    let _ =
        crate::mods::enrich::enrich_untracked(instance_root, modrinth_base, cf_base, cf_key).await;
    let untracked = match crate::mods::installed::read_or_empty(instance_root).await {
        Ok(state) => state.mods.iter().filter(|m| m.source.is_none()).count() as u32,
        Err(_) => 0,
    };
    Ok(untracked)
}

/// Run a full import. Creates the instance, copies the selected
/// categories, recovers mod identities, and finalizes. On a mandatory-
/// phase failure (instance create, or Mods copy when Mods selected) the
/// half-built instance is deleted (rollback). Best-effort failures
/// (other categories, enrich) are tolerated and reflected in the result.
///
/// Returns the new instance id plus the number of copied jars whose identity
/// could not be recovered. That count is a RESULT, not progress, so it rides
/// this return value — `ImportProgress::Done` carries no payload.
#[allow(clippy::too_many_arguments)]
pub async fn run_import(
    app: &tauri::AppHandle,
    foreign: &ForeignInstance,
    plan: &ImportPlan,
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
    // `Send + Sync` so the `#[tauri::command]` future built around this in
    // `launcher_import_run` stays `Send` (`emit` is held across an `.await`).
    // Mirrors `modpack::import::import`'s `on_progress` bound.
    emit: &(dyn Fn(ImportProgress) + Send + Sync),
) -> Result<(String, u32)> {
    use crate::instances;
    use crate::paths;

    emit(ImportProgress::CreatingInstance {
        name: plan.name.clone(),
    });
    let provenance = ImportProvenance {
        launcher: foreign.source,
        source_name: foreign.name.clone(),
        source_path: foreign.root.to_string_lossy().into_owned(),
        imported_unix_ms: now_unix_ms(),
    };
    let created = instances::create_instance(
        app,
        plan.name.clone(),
        plan.mc_version.clone(),
        plan.loader,
        plan.loader_version.clone(),
        Some(plan.max_heap_mb),
        instances::schema::PackOrigin::default(),
        Some(provenance),
        None,
    )?;
    let id = created.id;

    // Heap travels with the create above; only the jvm args still need a write.
    let _ = instances::set_instance_jvm_args(app, &id, plan.extra_jvm_args.clone());

    let instance_root =
        paths::instance_dir(app, &id).map_err(|e| Error::io("<instance_dir>", e))?;
    let dst_mc = paths::minecraft_dir(app, &id).map_err(|e| Error::io("<minecraft_dir>", e))?;

    // Copy categories. Mods is mandatory if selected — its failure rolls back.
    let mods_selected = plan.copy_categories.contains(&ContentCategory::Mods);
    for &cat in &plan.copy_categories {
        let res = copy_category(&foreign.minecraft_dir, &dst_mc, cat, &mut |cur, tot| {
            emit(ImportProgress::Copying {
                category: cat,
                current: cur,
                total: tot,
            });
        });
        if let Err(e) = res {
            if cat == ContentCategory::Mods {
                // Rollback the half-built instance. Remove the directory
                // directly rather than via `delete_instance`, which
                // returns `Err(LastInstance)` (a silent no-op) when the
                // import is the only instance on disk — the common
                // first-action-after-install case. The imported instance
                // is not active during `run_import`, so a direct dir
                // removal is safe; a stale active-pointer self-heals in
                // `get_active_instance`.
                let _ = std::fs::remove_dir_all(&instance_root);
                return Err(e);
            }
            // best-effort category: tolerate, continue.
        }
    }

    // Recover identities for copied mods (shared with the server→instance flow).
    let mut untracked = 0u32;
    if mods_selected {
        emit(ImportProgress::RecoveringIdentities);
        untracked = adopt_copied_jars(
            &instance_root,
            &dst_mc.join("mods"),
            &foreign.known_mods,
            modrinth_base,
            cf_base,
            cf_key,
        )
        .await?;
    }

    // Phase marker only — `untracked` rides the return value.
    emit(ImportProgress::Done);
    Ok((id, untracked))
}

/// SHA-1 (hex, lowercase) of every `*.jar` directly under `dir`.
fn hash_jars(dir: &Path) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "jar").unwrap_or(false))
        .filter_map(|p| {
            let bytes = std::fs::read(&p).ok()?;
            let filename = p.file_name()?.to_string_lossy().into_owned();
            Some((filename, sha1_hex(&bytes)))
        })
        .collect()
}

/// SHA-1 of `bytes` as a lowercase hex string. Matches the digest the
/// installed-mods registry and `enrich`/`install` use (`sha1` + `hex`).
fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

/// Wall-clock milliseconds since the Unix epoch as `f64` — matches the
/// `created_unix_ms` / specta `f64` convention used across instance files.
fn now_unix_ms() -> f64 {
    crate::instances::unix_ms_f64()
}

/// RFC-3339 timestamp for `installed_at`, matching the format the
/// installed-mods registry already writes (`chrono::Utc::now().to_rfc3339()`).
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::import::model::ContentCategory;

    #[tokio::test]
    async fn adopt_registers_jars_and_counts_untracked_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_root = tmp.path();
        let mods_dir = instance_root.join(".minecraft/mods");
        std::fs::create_dir_all(&mods_dir).unwrap();
        std::fs::write(mods_dir.join("a.jar"), b"AAA").unwrap();
        std::fs::write(mods_dir.join("b.jar"), b"BBB").unwrap();

        // Port 0 is never listened on — the connect is refused immediately, so
        // enrich_untracked returns Err(...) which adopt_copied_jars silences.
        // Both jars therefore stay untracked.
        let untracked = adopt_copied_jars(
            instance_root,
            &mods_dir,
            &[],
            "http://127.0.0.1:0",
            "http://127.0.0.1:0",
            None,
        )
        .await
        .unwrap();

        assert_eq!(untracked, 2);
        let state = crate::mods::installed::read_or_empty(instance_root)
            .await
            .unwrap();
        assert_eq!(state.mods.len(), 2);
        assert!(state.mods.iter().all(|m| m.source.is_none()));
    }

    fn write(p: &std::path::Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn copies_a_category_dir_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("mods/a.jar"), "AAA");
        write(&src_mc.join("mods/sub/b.jar"), "BBB");
        let dst_mc = tmp.path().join("dst/.minecraft");

        let report =
            copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |_, _| {}).unwrap();

        assert_eq!(report, 2);
        assert_eq!(
            std::fs::read_to_string(dst_mc.join("mods/a.jar")).unwrap(),
            "AAA"
        );
        assert_eq!(
            std::fs::read_to_string(dst_mc.join("mods/sub/b.jar")).unwrap(),
            "BBB"
        );
    }

    #[test]
    fn copies_options_txt_single_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("options.txt"), "version:3465");
        let dst_mc = tmp.path().join("dst/.minecraft");
        copy_category(
            &src_mc,
            &dst_mc,
            ContentCategory::OptionsTxt,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dst_mc.join("options.txt")).unwrap(),
            "version:3465"
        );
    }

    #[test]
    fn leaves_source_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("mods/a.jar"), "AAA");
        let before = std::fs::read(src_mc.join("mods/a.jar")).unwrap();
        let dst_mc = tmp.path().join("dst/.minecraft");
        copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |_, _| {}).unwrap();
        let after = std::fs::read(src_mc.join("mods/a.jar")).unwrap();
        assert_eq!(before, after);
        assert!(
            src_mc.join("mods/a.jar").exists(),
            "source file must survive"
        );
    }

    #[test]
    fn mods_copy_skips_launcher_injected_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("mods/sodium.jar"), "real");
        write(
            &src_mc.join("mods/tl_skin_cape_forge_1.21.1-1.39.jar"),
            "junk",
        );
        let dst_mc = tmp.path().join("dst/.minecraft");

        let copied =
            copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |_, _| {}).unwrap();

        assert_eq!(copied, 1, "only the real mod is copied");
        assert!(dst_mc.join("mods/sodium.jar").exists());
        assert!(
            !dst_mc
                .join("mods/tl_skin_cape_forge_1.21.1-1.39.jar")
                .exists(),
            "TLauncher-injected skin/cape mod must not be copied"
        );
    }

    #[test]
    fn builds_installed_records_using_known_identity_then_falls_back() {
        use crate::instances::import::model::KnownMod;
        use crate::mods::platform::ModSource;

        let known = vec![KnownMod {
            filename: "sodium.jar".into(),
            source: ModSource::Modrinth,
            project_id: "AANobbMI".into(),
            version_id: Some("v1".into()),
        }];
        // sodium matches a known identity; mystery.jar does not.
        let recs = build_installed_records(
            &[
                ("sodium.jar".to_string(), "HASH_SODIUM".to_string()),
                ("mystery.jar".to_string(), "HASH_MYSTERY".to_string()),
            ],
            &known,
            "2026-06-14T00:00:00Z",
        );
        let sodium = recs.iter().find(|m| m.filename == "sodium.jar").unwrap();
        assert_eq!(sodium.source, Some(ModSource::Modrinth));
        assert_eq!(sodium.project_id.as_deref(), Some("AANobbMI"));
        let mystery = recs.iter().find(|m| m.filename == "mystery.jar").unwrap();
        assert_eq!(mystery.source, None);
        assert!(
            !mystery.enrich_attempted,
            "untracked jar left for hash-enrich pass"
        );
    }
}
