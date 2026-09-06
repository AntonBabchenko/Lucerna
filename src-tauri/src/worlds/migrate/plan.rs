//! Plan phase (§6, §5 plan-time prediction): read-only. Nothing here writes
//! into either instance; `mods::installed::list` may rewrite its own JSON
//! registry sidecar on reconcile, which is that module's documented behaviour.

use std::path::Path;

use crate::datapacks::level_dat::{self, WorldVersion};
use crate::datapacks::{compat, library, library_dir_at};
use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;
use crate::mods::installed;

use super::{
    DatapackPlan, DatapackResult, LeftReason, MigrationLocations, MigrationPlan, UnknownReason,
    VersionVerdict,
};

/// `Ok(Ok(v))`: level.dat read and parsed. `Ok(Err(reason))`: the plan still
/// succeeds with a typed unknown. `Err(_)`: the plan fails — `WorldInUse`, or a
/// read failure that is not NotFound.
type WorldVersionRead = std::result::Result<WorldVersion, UnknownReason>;

/// Build the plan for `loc.world_folder` (in `loc.src_saves`) → the target
/// instance. `src_mc` is logged for context only; the world's own version key
/// is `level.dat`'s DataVersion (A6). `dst_mc == ""` is the fresh-install state.
pub async fn plan_migration_at(
    loc: &MigrationLocations,
    versions_dir: &Path,
    src_mc: &str,
    dst_mc: &str,
    src_loader: LoaderKind,
    dst_loader: LoaderKind,
) -> Result<MigrationPlan> {
    let world_dir = loc.src_saves.join(&loc.world_folder);

    // level.dat may gzip-inflate up to 64 MB (`level_dat::parse`); off the
    // async thread, like every other blocking piece of this core (A10).
    let world_c = world_dir.clone();
    let world_version = tokio::task::spawn_blocking(move || read_world_version(&world_c))
        .await
        .map_err(|e| Error::io(world_dir.display().to_string(), format!("join: {e}")))??;

    let target_key = if dst_mc.is_empty() {
        // Irrelevant: an empty target version is answered before the jar is
        // consulted (`verdict_of`). `JarUnavailable` keeps the type honest.
        compat::JarWorldVersion::JarUnavailable
    } else {
        let versions_c = versions_dir.to_path_buf();
        let dst_mc_c = dst_mc.to_string();
        tokio::task::spawn_blocking(move || compat::world_version_of_jar(&versions_c, &dst_mc_c))
            .await
            .map_err(|e| Error::io(versions_dir.display().to_string(), format!("join: {e}")))?
    };

    let verdict = verdict_of(&world_version, dst_mc, target_key);
    if let VersionVerdict::Unknown { reason } = &verdict {
        crate::diag!(
            "world migration plan: {} ({src_mc} -> {dst_mc}): version verdict unknown ({reason:?})",
            loc.world_folder
        );
    }

    let src_mods = installed::list(&loc.src_root).await?;
    let dst_mods = installed::list(&loc.dst_root).await?;
    let mods_missing_in_target = installed::missing_in(&src_mods, &dst_mods);

    let dp_dir = world_dir.join("datapacks");
    let library_dir = library_dir_at(&loc.dst_root);
    let (datapacks, datapacks_folders) =
        tokio::task::spawn_blocking(move || predict_datapacks(&dp_dir, &library_dir))
            .await
            .map_err(|e| Error::io(world_dir.display().to_string(), format!("join: {e}")))??;

    Ok(MigrationPlan {
        world_version_name: world_version.as_ref().ok().and_then(|v| v.name.clone()),
        verdict,
        source_loader: src_loader,
        target_loader: dst_loader,
        mods_missing_in_target,
        datapacks,
        datapacks_folders,
    })
}

fn read_world_version(world_dir: &Path) -> Result<WorldVersionRead> {
    // `symlink_metadata`, not `exists()`: NotFound is the only "absent"
    // answer. Any other stat failure is "could not tell" and falls through to
    // `read_at`, whose errno mapping turns a held-open file into `WorldInUse`
    // and everything else into `Io` — the restrictive direction (Fallback
    // discipline Q1/Q2): a level.dat that cannot be inspected never plans as
    // "this world has no level.dat".
    if let Err(e) = std::fs::symlink_metadata(world_dir.join("level.dat")) {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Ok(Err(UnknownReason::NoLevelDat));
        }
    }
    match level_dat::read_at(world_dir) {
        Ok((root, _framing)) => Ok(Ok(level_dat::version_of(&root))),
        Err(Error::LevelDatParse { reason }) => {
            crate::diag!(
                "world migration plan: {} level.dat unreadable: {reason}",
                world_dir.display()
            );
            Ok(Err(UnknownReason::Unreadable))
        }
        Err(other) => Err(other),
    }
}

/// §6 precedence: a world-side reason (no / unreadable level.dat) first, then
/// the target side (no version set, jar not installed), then a world that
/// records no DataVersion; only then the integer comparison.
fn verdict_of(
    world: &WorldVersionRead,
    dst_mc: &str,
    target_key: compat::JarWorldVersion,
) -> VersionVerdict {
    let world = match world {
        Ok(v) => v,
        Err(reason) => return VersionVerdict::Unknown { reason: *reason },
    };
    if dst_mc.is_empty() {
        return VersionVerdict::Unknown {
            reason: UnknownReason::TargetVersionUnset,
        };
    }
    let target = match target_key {
        compat::JarWorldVersion::Version(n) => n,
        // No jar, or one that cannot be opened: "install or repair it".
        compat::JarWorldVersion::JarUnavailable => {
            return VersionVerdict::Unknown {
                reason: UnknownReason::TargetNotInstalled,
            }
        }
        // An installed pre-1.14 client (the real 1.12.2 jar has no
        // version.json): the target's DataVersion is unknowable — say so,
        // never "install it first" about an installed version.
        compat::JarWorldVersion::NotRecorded => {
            return VersionVerdict::Unknown {
                reason: UnknownReason::TargetNotRecorded,
            }
        }
    };
    let Some(data_version) = world.data_version else {
        return VersionVerdict::Unknown {
            reason: UnknownReason::NotRecorded,
        };
    };
    match data_version.cmp(&target) {
        std::cmp::Ordering::Equal => VersionVerdict::Same,
        std::cmp::Ordering::Less => VersionVerdict::WillUpgrade,
        std::cmp::Ordering::Greater => VersionVerdict::WorldIsNewer,
    }
}

/// Regular `*.zip` files (case-insensitive) directly under the world's
/// `datapacks/` get a prediction; directories are counted as folder packs;
/// symlinks and other files are ignored (the §5 scope rule).
fn predict_datapacks(dp_dir: &Path, library_dir: &Path) -> Result<(Vec<DatapackPlan>, u32)> {
    let entries = match std::fs::read_dir(dp_dir) {
        Ok(entries) => entries,
        // A world without a datapacks/ folder has zero packs — the one benign
        // failure. Anything else is "could not tell" and fails the plan.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(Error::io(dp_dir.display().to_string(), e)),
    };
    let mut packs = Vec::new();
    let mut folders: u32 = 0;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dp_dir.display().to_string(), e))?;
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            // `copy_tree` skips it; the stage step counts it in `links_skipped`.
            continue;
        }
        if ft.is_dir() {
            folders = folders.saturating_add(1);
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        if !ft.is_file() || !name.to_ascii_lowercase().ends_with(".zip") {
            continue;
        }
        let predicted = predict_one(&entry.path(), &library_dir.join(&name));
        packs.push(DatapackPlan {
            filename: name,
            predicted,
        });
    }
    // `read_dir` order is unspecified; the dialog and the tests want one order.
    packs.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok((packs, folders))
}

fn held_by_a_different_pack() -> DatapackResult {
    DatapackResult::LeftAsCopy {
        reason: LeftReason::NameHeldByDifferentPack,
    }
}

/// §5 name-first rule at plan time. Absent in the library ⇒ `Adopted`;
/// present with identical on-disk bytes ⇒ `Linked`; present with different
/// bytes — or anything that cannot be read — ⇒ `LeftAsCopy`.
fn predict_one(world_file: &Path, library_file: &Path) -> DatapackResult {
    match library_file.try_exists() {
        Ok(false) => return DatapackResult::Adopted,
        Ok(true) => {}
        // "Could not tell" reads as present-with-different-bytes: the
        // restrictive direction (Fallback discipline Q1/Q2). Predicting
        // `Adopted` here would promise a library write the migration may
        // then refuse.
        Err(_) => return held_by_a_different_pack(),
    }
    // Both files are hashed ON DISK (§5.3): the registry keeps names without
    // re-hashing. An unreadable file on either side cannot be proven
    // identical, and §5.4 classes it with "differs".
    let (Ok(ours), Ok(theirs)) = (std::fs::read(world_file), std::fs::read(library_file)) else {
        return held_by_a_different_pack();
    };
    if library::sha1_hex(&ours) == library::sha1_hex(&theirs) {
        DatapackResult::Linked
    } else {
        held_by_a_different_pack()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write as _;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A gzip-framed level.dat with `Data.Version.{Id,Name}` when either is
    /// given; with neither, a pre-1.10 shape that records no version at all.
    fn level_dat_bytes(id: Option<i32>, name: Option<&str>) -> Vec<u8> {
        let mut data = HashMap::new();
        data.insert("LevelName".to_string(), Value::String("Survival".into()));
        if id.is_some() || name.is_some() {
            let mut version = HashMap::new();
            if let Some(id) = id {
                version.insert("Id".to_string(), Value::Int(id));
            }
            if let Some(name) = name {
                version.insert("Name".to_string(), Value::String(name.into()));
            }
            data.insert("Version".to_string(), Value::Compound(version));
        }
        let mut root = HashMap::new();
        root.insert("Data".to_string(), Value::Compound(data));
        level_dat::serialize(&Value::Compound(root), level_dat::Framing::Gzip).unwrap()
    }

    /// A client-jar lookalike: a zip whose `version.json` carries (or omits)
    /// `world_version` — Mojang's field, the same shape `compat.rs` tests use.
    fn jar_with_world_version(world_version: Option<i32>) -> Vec<u8> {
        let body = match world_version {
            Some(v) => format!(r#"{{"id":"1.21.5","world_version":{v}}}"#),
            None => r#"{"id":"1.21.5"}"#.to_string(),
        };
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("version.json", opts).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    struct Fx {
        _td: tempfile::TempDir,
        loc: MigrationLocations,
        versions_dir: PathBuf,
        world: PathBuf,
    }

    fn fixture() -> Fx {
        let td = tempdir().unwrap();
        let src_root = td.path().join("instances").join("Src");
        let dst_root = td.path().join("instances").join("Dst");
        let loc = MigrationLocations {
            src_saves: src_root.join(".minecraft").join("saves"),
            src_backups_root: src_root.join("backups"),
            src_root: src_root.clone(),
            dst_saves: dst_root.join(".minecraft").join("saves"),
            dst_backups_root: dst_root.join("backups"),
            dst_root: dst_root.clone(),
            world_folder: "Survival".into(),
            target_instance_name: "Target".into(),
        };
        let world = loc.src_saves.join("Survival");
        fs::create_dir_all(&world).unwrap();
        fs::create_dir_all(&dst_root).unwrap();
        let versions_dir = td.path().join("versions");
        Fx {
            _td: td,
            loc,
            versions_dir,
            world,
        }
    }

    fn install_jar(versions_dir: &Path, mc: &str, world_version: Option<i32>) {
        let dir = versions_dir.join(mc);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{mc}.jar")),
            jar_with_world_version(world_version),
        )
        .unwrap();
    }

    async fn plan(fx: &Fx, dst_mc: &str) -> Result<MigrationPlan> {
        plan_migration_at(
            &fx.loc,
            &fx.versions_dir,
            "1.20.1",
            dst_mc,
            LoaderKind::Fabric,
            LoaderKind::Fabric,
        )
        .await
    }

    // 1.20.1 = DataVersion 3465, 1.21.5 = 4325 (Minecraft's own integers).

    #[tokio::test]
    async fn same_when_the_keys_are_equal() {
        let fx = fixture();
        fs::write(
            fx.world.join("level.dat"),
            level_dat_bytes(Some(3465), Some("1.20.1")),
        )
        .unwrap();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        let p = plan(&fx, "1.20.1").await.unwrap();
        assert_eq!(p.verdict, VersionVerdict::Same);
        assert_eq!(p.world_version_name.as_deref(), Some("1.20.1"));
        assert_eq!(p.source_loader, LoaderKind::Fabric);
    }

    #[tokio::test]
    async fn will_upgrade_when_the_world_is_older() {
        let fx = fixture();
        fs::write(
            fx.world.join("level.dat"),
            level_dat_bytes(Some(3465), Some("1.20.1")),
        )
        .unwrap();
        install_jar(&fx.versions_dir, "1.21.5", Some(4325));
        assert_eq!(
            plan(&fx, "1.21.5").await.unwrap().verdict,
            VersionVerdict::WillUpgrade
        );
    }

    #[tokio::test]
    async fn world_is_newer_when_the_world_is_newer() {
        let fx = fixture();
        fs::write(
            fx.world.join("level.dat"),
            level_dat_bytes(Some(4325), Some("1.21.5")),
        )
        .unwrap();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        assert_eq!(
            plan(&fx, "1.20.1").await.unwrap().verdict,
            VersionVerdict::WorldIsNewer
        );
    }

    #[tokio::test]
    async fn no_level_dat_plans_as_unknown_not_as_an_error() {
        let fx = fixture();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        let p = plan(&fx, "1.20.1").await.unwrap();
        assert_eq!(
            p.verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::NoLevelDat
            }
        );
        assert_eq!(p.world_version_name, None);
    }

    #[tokio::test]
    async fn an_unreadable_level_dat_plans_as_unknown() {
        let fx = fixture();
        fs::write(fx.world.join("level.dat"), b"absolutely not nbt").unwrap();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        assert_eq!(
            plan(&fx, "1.20.1").await.unwrap().verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::Unreadable
            }
        );
    }

    #[tokio::test]
    async fn a_world_without_a_recorded_version_is_not_recorded() {
        let fx = fixture();
        fs::write(fx.world.join("level.dat"), level_dat_bytes(None, None)).unwrap();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        assert_eq!(
            plan(&fx, "1.20.1").await.unwrap().verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::NotRecorded
            }
        );
    }

    #[tokio::test]
    async fn an_empty_target_version_is_target_version_unset() {
        let fx = fixture();
        fs::write(
            fx.world.join("level.dat"),
            level_dat_bytes(Some(3465), Some("1.20.1")),
        )
        .unwrap();
        assert_eq!(
            plan(&fx, "").await.unwrap().verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::TargetVersionUnset
            }
        );
    }

    #[tokio::test]
    async fn a_missing_jar_is_target_not_installed() {
        let fx = fixture();
        fs::write(
            fx.world.join("level.dat"),
            level_dat_bytes(Some(3465), Some("1.20.1")),
        )
        .unwrap();
        assert_eq!(
            plan(&fx, "1.21.5").await.unwrap().verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::TargetNotInstalled
            }
        );
    }

    /// An INSTALLED jar that records no `world_version` (the real 1.12.2
    /// client has no `version.json` at all) is not "not installed": the
    /// verdict must say the target's DataVersion is unknowable, never
    /// "install it first" about an installed version.
    #[tokio::test]
    async fn an_installed_jar_without_world_version_is_target_not_recorded() {
        let fx = fixture();
        fs::write(
            fx.world.join("level.dat"),
            level_dat_bytes(Some(3465), Some("1.20.1")),
        )
        .unwrap();
        install_jar(&fx.versions_dir, "1.21.5", None);
        assert_eq!(
            plan(&fx, "1.21.5").await.unwrap().verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::TargetNotRecorded
            }
        );
    }

    #[tokio::test]
    async fn a_world_side_unknown_wins_over_a_target_side_one() {
        let fx = fixture();
        assert_eq!(
            plan(&fx, "").await.unwrap().verdict,
            VersionVerdict::Unknown {
                reason: UnknownReason::NoLevelDat
            }
        );
    }

    /// A DIRECTORY at level.dat's path: the stat succeeds (so this is not
    /// "absent") and the read fails with something other than NotFound on
    /// every platform — Windows reports a directory open as access denied
    /// (5), which `read_at` maps to `WorldInUse`; POSIX reports EISDIR → `Io`.
    /// Either way the plan must FAIL, never answer `NoLevelDat` out of
    /// ignorance.
    #[tokio::test]
    async fn a_level_dat_that_cannot_be_read_fails_the_plan() {
        let fx = fixture();
        fs::create_dir(fx.world.join("level.dat")).unwrap();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        let err = plan(&fx, "1.20.1").await.unwrap_err();
        assert!(
            matches!(err, Error::Io { .. } | Error::WorldInUse { .. }),
            "got: {err:?}"
        );
    }

    #[tokio::test]
    async fn mods_missing_is_zero_when_the_source_has_no_mods_whatever_the_loaders() {
        let fx = fixture();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        let p = plan_migration_at(
            &fx.loc,
            &fx.versions_dir,
            "1.20.1",
            "1.20.1",
            LoaderKind::Fabric,
            LoaderKind::Forge,
        )
        .await
        .unwrap();
        assert_eq!(p.mods_missing_in_target, 0);
        assert_eq!(p.target_loader, LoaderKind::Forge);
    }

    #[tokio::test]
    async fn mods_missing_counts_source_jars_the_target_does_not_hold() {
        let fx = fixture();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        // `installed::list` synthesises a row (sha1, no project_id) for every
        // jar on disk, so the pair is matched by sha1: alpha is in both,
        // beta only in the source.
        let src_mods = fx.loc.src_root.join(".minecraft").join("mods");
        let dst_mods = fx.loc.dst_root.join(".minecraft").join("mods");
        fs::create_dir_all(&src_mods).unwrap();
        fs::create_dir_all(&dst_mods).unwrap();
        fs::write(src_mods.join("alpha.jar"), b"ALPHA-BYTES").unwrap();
        fs::write(src_mods.join("beta.jar"), b"BETA-BYTES").unwrap();
        fs::write(dst_mods.join("alpha.jar"), b"ALPHA-BYTES").unwrap();
        assert_eq!(plan(&fx, "1.20.1").await.unwrap().mods_missing_in_target, 1);
    }

    #[tokio::test]
    async fn datapacks_are_predicted_by_name_then_bytes_and_folders_are_counted() {
        let fx = fixture();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        let dp = fx.world.join("datapacks");
        fs::create_dir_all(dp.join("folderpack").join("data")).unwrap();
        fs::write(dp.join("keep.zip"), b"same bytes").unwrap();
        fs::write(dp.join("clash.zip"), b"world has v1").unwrap();
        fs::write(dp.join("new.zip"), b"only in the world").unwrap();
        fs::write(dp.join("notes.txt"), b"ignored").unwrap();
        let lib = library_dir_at(&fx.loc.dst_root);
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("keep.zip"), b"same bytes").unwrap();
        fs::write(lib.join("clash.zip"), b"library has v2").unwrap();

        let p = plan(&fx, "1.20.1").await.unwrap();

        assert_eq!(p.datapacks_folders, 1);
        assert_eq!(
            p.datapacks,
            vec![
                DatapackPlan {
                    filename: "clash.zip".into(),
                    predicted: DatapackResult::LeftAsCopy {
                        reason: LeftReason::NameHeldByDifferentPack
                    },
                },
                DatapackPlan {
                    filename: "keep.zip".into(),
                    predicted: DatapackResult::Linked,
                },
                DatapackPlan {
                    filename: "new.zip".into(),
                    predicted: DatapackResult::Adopted,
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_missing_datapacks_folder_is_zero_packs_and_an_upper_case_zip_counts() {
        let fx = fixture();
        install_jar(&fx.versions_dir, "1.20.1", Some(3465));
        let p = plan(&fx, "1.20.1").await.unwrap();
        assert!(p.datapacks.is_empty());
        assert_eq!(p.datapacks_folders, 0);

        let dp = fx.world.join("datapacks");
        fs::create_dir_all(&dp).unwrap();
        fs::write(dp.join("Loud.ZIP"), b"x").unwrap();
        let p = plan(&fx, "1.20.1").await.unwrap();
        assert_eq!(p.datapacks.len(), 1);
        assert_eq!(p.datapacks[0].filename, "Loud.ZIP");
        assert_eq!(p.datapacks[0].predicted, DatapackResult::Adopted);
    }
}
