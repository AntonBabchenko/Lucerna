//! Modpack export — the inverse of `import`. Turns an instance back into
//! a Modrinth `.mrpack` or CurseForge `.zip`. Read-only except for the
//! output file; the source instance is never modified.

pub mod assembly;
pub mod classify;
pub mod manifest;
pub mod types;

pub use types::{
    ExportMetadata, ExportModInfo, ExportMode, ExportOptions, ExportPreview, ModpackExportProgress,
};

use std::path::Path;

use crate::error::Error;
use crate::instances::schema::LoaderKind;
use crate::mods::modpack::schema::ModpackFormat;
use crate::mods::platform::InstalledMod;

use assembly::{
    collect_dir_entries, hash_file, resolve_download_url, write_archive, ZipEntry, ZipSource,
};
use manifest::{build_cf_manifest, build_mrpack_index, CfRef, MrpackRef};

/// Emit progress without caring whether a channel is wired (tests pass a no-op).
pub type ProgressSink<'a> = dyn Fn(ModpackExportProgress) + Send + Sync + 'a;

/// Run a full export. `instance_root` is the instance directory (contains
/// `.minecraft/`). `enabled_mods` is the reconciled, ENABLED-only mod list.
/// Reads files only; the single write is `dest`.
pub async fn run_export(
    instance_root: &Path,
    mc_version: &str,
    loader: LoaderKind,
    loader_version: Option<&str>,
    enabled_mods: &[InstalledMod],
    opts: &ExportOptions,
    dest: &Path,
    progress: &ProgressSink<'_>,
) -> Result<(), Error> {
    let mc_dir = instance_root.join(".minecraft");
    let mods_dir = mc_dir.join("mods");

    let (referenced, unresolvable) = classify::classify(opts.format, opts.mode, enabled_mods);

    // Decide which unresolvable mods get bundled. Full mode -> all; otherwise
    // only the user-chosen `bundle_shas`.
    let bundle_set: std::collections::HashSet<String> = opts
        .bundle_shas
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let to_bundle: Vec<&InstalledMod> = unresolvable
        .iter()
        .copied()
        .filter(|m| {
            matches!(opts.mode, ExportMode::Full)
                || bundle_set.contains(&m.sha1.to_ascii_lowercase())
        })
        .collect();

    // --- Resolve URLs for referenced mods (Resolving phase) ---
    let total_ref = referenced.len() as u32;
    let mut mrpack_refs: Vec<MrpackRef> = Vec::new();
    let mut cf_refs: Vec<CfRef> = Vec::new();
    // Referenced mods that fail resolution at runtime fall back to bundling.
    let mut fallback_bundle: Vec<&InstalledMod> = Vec::new();

    for (i, m) in referenced.iter().enumerate() {
        progress(ModpackExportProgress::Resolving {
            current: i as u32 + 1,
            total: total_ref,
        });

        // classify() guarantees referenced mods have source + both ids, but
        // handle gracefully rather than panicking if somehow they're missing.
        let (source, project_id, version_id) =
            match (m.source, m.project_id.as_deref(), m.version_id.as_deref()) {
                (Some(s), Some(pid), Some(vid)) => (s, pid, vid),
                _ => {
                    fallback_bundle.push(m);
                    continue;
                }
            };

        match opts.format {
            ModpackFormat::Curseforge => {
                // CF zip references CF mods by numeric ids only.
                match (project_id.parse::<u64>(), version_id.parse::<u64>()) {
                    (Ok(pid), Ok(fid)) => cf_refs.push(CfRef {
                        project_id: pid,
                        file_id: fid,
                    }),
                    _ => fallback_bundle.push(m),
                }
            }
            ModpackFormat::Modrinth => {
                match resolve_download_url(source, project_id, version_id).await {
                    Ok(Some(url)) => {
                        let jar = mods_dir.join(&m.filename);
                        let jar2 = jar.clone();
                        let (sha1, sha512, size) =
                            tokio::task::spawn_blocking(move || hash_file(&jar2))
                                .await
                                .map_err(|e| Error::ModpackExportFailed {
                                    details: format!("hash task panicked: {e}"),
                                })??;
                        mrpack_refs.push(MrpackRef {
                            path: format!("mods/{}", m.filename),
                            sha1,
                            sha512,
                            url,
                            size,
                        });
                    }
                    // Distribution disabled (Ok(None)) OR a resolve error (network, delisted)
                    // -> bundle the local jar instead of aborting.
                    Ok(None) | Err(_) => fallback_bundle.push(m),
                }
            }
            // FTB: pack-managed source — export is unsupported (no upload target).
            // classify() already routes all FTB mods to unresolvable/bundle; this
            // arm is unreachable in practice but required for exhaustiveness.
            ModpackFormat::Ftb => fallback_bundle.push(m),
        }
    }

    // --- Assemble override entries (Bundling phase) ---
    let mut entries: Vec<ZipEntry> = Vec::new();

    // Bundled mod jars (unresolvable-chosen + runtime fallbacks).
    let bundled_jars: Vec<&InstalledMod> = to_bundle
        .into_iter()
        .chain(fallback_bundle.into_iter())
        .collect();
    let total_bundle = bundled_jars.len() as u32;
    for (i, m) in bundled_jars.iter().enumerate() {
        progress(ModpackExportProgress::Bundling {
            current: i as u32 + 1,
            total: total_bundle,
        });
        let jar = mods_dir.join(&m.filename);
        if jar.exists() {
            entries.push(ZipEntry {
                archive_path: format!("overrides/mods/{}", m.filename),
                source: ZipSource::File(jar),
            });
        }
    }

    // Content categories -> overrides/ (whole-directory copies). Resourcepacks
    // / shaderpacks / configs / saves have no provenance registry, so they
    // always travel as overrides regardless of mode.
    if opts.include_config {
        entries.extend(collect_dir_entries(
            &mc_dir.join("config"),
            "overrides/config",
        )?);
    }
    if opts.include_resourcepacks {
        entries.extend(collect_dir_entries(
            &mc_dir.join("resourcepacks"),
            "overrides/resourcepacks",
        )?);
    }
    if opts.include_shaderpacks {
        entries.extend(collect_dir_entries(
            &mc_dir.join("shaderpacks"),
            "overrides/shaderpacks",
        )?);
    }
    if opts.include_worlds {
        entries.extend(collect_dir_entries(
            &mc_dir.join("saves"),
            "overrides/saves",
        )?);
    }

    // --- Manifest (Writing phase) ---
    progress(ModpackExportProgress::Writing);
    let (manifest_name, manifest_json) = match opts.format {
        ModpackFormat::Modrinth => (
            "modrinth.index.json".to_string(),
            build_mrpack_index(
                &opts.metadata,
                mc_version,
                loader,
                loader_version,
                &mrpack_refs,
            )
            .map_err(|e| Error::ModpackExportFailed {
                details: e.to_string(),
            })?,
        ),
        ModpackFormat::Curseforge => (
            "manifest.json".to_string(),
            build_cf_manifest(&opts.metadata, mc_version, loader, loader_version, &cf_refs)
                .map_err(|e| Error::ModpackExportFailed {
                    details: e.to_string(),
                })?,
        ),
        // FTB: pack-managed source — export is not supported (FTB has no
        // user-upload target). The UI gates export behind SourceCaps.can_export;
        // returning a typed error here guards against mis-routing.
        ModpackFormat::Ftb => {
            return Err(Error::ModpackExportFailed {
                details: "FTB packs cannot be exported".into(),
            });
        }
    };
    entries.push(ZipEntry {
        archive_path: manifest_name,
        source: ZipSource::Bytes(manifest_json.into_bytes()),
    });

    let dest_owned = dest.to_path_buf();
    tokio::task::spawn_blocking(move || write_archive(&dest_owned, &entries))
        .await
        .map_err(|e| Error::ModpackExportFailed {
            details: format!("write task panicked: {e}"),
        })??;
    progress(ModpackExportProgress::Done {
        path: dest.display().to_string(),
    });
    Ok(())
}
