//! Phase 2 of a modpack update, all or nothing (spec 2026-10-04 §4.3): set the
//! old files aside, place the new ones, write the pack record, then commit — or
//! undo every step when any of them fails, so a pack that cannot finish its
//! update stays exactly on the version it was.
//!
//! It used to continue past a failed file and record the new version anyway,
//! because by then the old files were already deleted and stopping would have
//! left less behind. Setting the old files aside (`mods::txn`) removes that
//! reason: stopping now loses nothing.
//!
//! Path-based (no `AppHandle`) so it is driven directly in tests; the command
//! keeps the maintenance claim, phase 1 (the downloads) and the report.

use std::collections::HashSet;
use std::path::Path;

use tokio::fs;

use crate::error::Error;
use crate::mods::install::{self, ProgressFn};
use crate::mods::installed::{self, PackOrigin, PackOriginFile};
use crate::mods::modpack::import;
use crate::mods::modpack::path_safety::is_safe_relative_path;
use crate::mods::modpack::schema::{
    InertLoaderJar, ModpackFile, ModpackSummary, ModpackUpdateDiff,
};
use crate::mods::platform::InstalledMod;
use crate::mods::txn::{self, Plan, PriorInstance, StageEntry, TxnKind};
use crate::tasks::{DetailOutcome, TaskDetail};

/// Everything phase 2 needs, read by the command before phase 1.
pub struct PackApply<'a> {
    pub data_dir: &'a Path,
    pub inst_root: &'a Path,
    pub diff: &'a ModpackUpdateDiff,
    pub summary: &'a ModpackSummary,
    pub old_origin: &'a PackOrigin,
    /// SHA-1s (lowercase) of updated mods the user had switched off.
    pub carry_disabled: &'a [String],
    pub new_version_id: &'a str,
}

/// What a committed update produced: one report row per placed file, and the
/// jars that will not load under the pack's loader.
pub struct AppliedUpdate {
    pub details: Vec<TaskDetail>,
    pub inert_loader_jars: Vec<InertLoaderJar>,
}

/// Run phase 2 inside a content transaction. It reports no progress of its
/// own: placing files from the warm cache takes milliseconds each, and phase 1
/// already counted them while downloading. A step that fails is undone: the
/// error is `ModpackUpdateRolledBack` (nothing was changed), or
/// `ContentUpdateRollbackIncomplete` when the undo itself left files behind.
/// Errors before `begin` changed nothing either, and a commit that cannot be
/// recorded is undone and returns that error as it is.
pub async fn apply_update_txn(
    a: PackApply<'_>,
    progress: &ProgressFn,
) -> Result<AppliedUpdate, Error> {
    let lock = txn::lock(a.inst_root).await;
    let rows = installed::list(a.inst_root).await?;
    let instance_json = a.inst_root.join("instance.json");
    let prior_instance = PriorInstance::of(&crate::instances::store::read_instance_json(
        &instance_json,
    )?);
    let plan = build_pack_plan(&a, &rows, prior_instance).await?;
    let txn = txn::begin(lock, plan).await?;
    match run(&txn, &a, progress).await {
        Ok(applied) => txn.finish(Ok(applied)).await,
        // Named "rolled back" only once the undo turned out clean: an undo that
        // left files behind reports the bare cause instead.
        Err(StepFailure { file_name, cause }) => Err(txn
            .rollback_as(cause, |cause| Error::ModpackUpdateRolledBack {
                file_name,
                cause: Box::new(cause),
            })
            .await),
    }
}

/// Where phase 2 stopped and why. `file_name` is the file it was setting aside
/// or placing; `None` while it wrote the pack record.
struct StepFailure {
    file_name: Option<String>,
    cause: Error,
}

impl StepFailure {
    fn at(file_name: Option<String>) -> impl FnOnce(Error) -> Self {
        move |cause| Self { file_name, cause }
    }
}

async fn run(
    txn: &txn::Txn,
    a: &PackApply<'_>,
    progress: &ProgressFn,
) -> Result<AppliedUpdate, StepFailure> {
    for entry in &txn.plan().stage {
        txn.stage(entry)
            .await
            .map_err(StepFailure::at(Some(file_name_of(&entry.rel))))?;
    }

    let to_place: Vec<&ModpackFile> = a
        .diff
        .added
        .iter()
        .chain(a.diff.updated.iter().map(|e| &e.new))
        .collect();
    let mut details = Vec::with_capacity(to_place.len());
    for f in &to_place {
        let row = place(a, f, progress)
            .await
            .map_err(StepFailure::at(Some(f.filename.clone())))?;
        details.push(row);
    }

    // The pack record: the new files[] plus the carried-over bundled entries.
    let bundled: Vec<PackOriginFile> = a
        .old_origin
        .files
        .iter()
        .filter(|f| f.url.is_empty())
        .cloned()
        .collect();
    let selected: Vec<&ModpackFile> = a
        .summary
        .files
        .iter()
        .filter(|f| !f.url.is_empty())
        .collect();
    let mut new_origin = import::build_pack_origin(
        a.summary,
        &selected,
        a.old_origin.project_id.clone(),
        &a.old_origin.project_name,
    );
    new_origin.files.extend(bundled);
    // The new mod set is in place, so the mods dir can be re-classified for jars
    // a loader change made inert — recomputing beats carrying a stale verdict.
    let inert_loader_jars = import::classify_inert_loader_jars(
        &installed::mods_dir(a.inst_root),
        a.summary.loader,
        &a.summary.game_version,
    );
    let new_origin =
        import::with_carried_notes(new_origin, a.old_origin.clone(), inert_loader_jars.clone());
    installed::set_pack_origin(a.inst_root, new_origin)
        .await
        .map_err(StepFailure::at(None))?;
    write_instance_fields(a).map_err(StepFailure::at(None))?;

    Ok(AppliedUpdate {
        details,
        inert_loader_jars,
    })
}

/// Place one new or changed pack file and return its report row.
async fn place(
    a: &PackApply<'_>,
    f: &ModpackFile,
    progress: &ProgressFn,
) -> Result<TaskDetail, Error> {
    if f.install_path.starts_with("mods/") {
        let mv = import::modpack_file_to_mod_version(f, &a.summary.game_version, a.summary.loader);
        let placed = install::install_one(a.data_dir, a.inst_root, mv, None, progress).await?;
        let outcome = match placed.placement {
            Some(placement) => DetailOutcome::Installed {
                fetched: placed.fetched,
                placement,
            },
            None => DetailOutcome::Unchanged,
        };
        // A mod the user had switched off stays off across the update.
        let sha = f.sha1.to_ascii_lowercase();
        if a.carry_disabled.contains(&sha) {
            install::disable(a.inst_root, &sha).await?;
        }
        Ok(import::modpack_file_detail(f, Some(&placed.sha1), outcome))
    } else {
        let asset = install::install_asset(
            a.data_dir,
            a.inst_root,
            &f.url,
            &f.sha1,
            f.size,
            &f.install_path,
            progress,
        )
        .await?;
        Ok(import::modpack_file_detail(
            f,
            Some(&f.sha1),
            DetailOutcome::Installed {
                fetched: asset.fetched,
                placement: asset.placement,
            },
        ))
    }
}

/// The five pack fields of `instance.json`, through the one field-applier
/// `instances::apply_pack_update_fields` (a Vanilla loader keeps no loader
/// version).
fn write_instance_fields(a: &PackApply<'_>) -> Result<(), Error> {
    let path = a.inst_root.join("instance.json");
    let mut inst = crate::instances::store::read_instance_json(&path)?;
    crate::instances::apply_pack_update_fields(
        &mut inst,
        a.summary.version.clone(),
        a.summary.game_version.clone(),
        a.summary.loader,
        a.summary.loader_version.clone(),
        a.new_version_id.to_string(),
    );
    crate::instances::store::write_instance_json(&path, &inst)
}

/// The whole plan, before anything is touched (spec §4.1, §4.3).
async fn build_pack_plan(
    a: &PackApply<'_>,
    rows: &[InstalledMod],
    prior_instance: PriorInstance,
) -> Result<Plan, Error> {
    let root = a.inst_root;
    let mut stage: Vec<StageEntry> = Vec::new();
    let mut staged: HashSet<String> = HashSet::new();
    for f in a
        .diff
        .removed
        .iter()
        .chain(a.diff.updated.iter().map(|e| &e.old))
    {
        // Two origin entries can resolve to one file (one jar recorded under two
        // projects): it is set aside once — a second move would find nothing.
        if let Some(entry) = stage_entry_for(root, rows, f).await? {
            if staged.insert(entry.rel.to_ascii_lowercase()) {
                stage.push(entry);
            }
        }
    }

    let mut create = Vec::new();
    for f in a
        .diff
        .added
        .iter()
        .chain(a.diff.updated.iter().map(|e| &e.new))
    {
        let rel = if f.install_path.starts_with("mods/") {
            format!("mods/{}", f.filename)
        } else {
            f.install_path.clone()
        };
        if !is_safe_relative_path(&rel) {
            return Err(Error::ModpackOverridesPathEscape {
                entry: f.install_path.clone(),
            });
        }
        if !rel.starts_with("mods/") && !staged.contains(&rel.to_ascii_lowercase()) {
            // `install_asset` writes over whatever is at its path — the user's
            // own config file, say. Set it aside so an undo brings it back.
            // Hashing discriminates on its own: no file is `None`, a read
            // that fails is an error that refuses the update untouched.
            let path = txn::content_path(root, &rel);
            if let Some(sha1) = txn::file_sha1(&path).await.map_err(|e| io_err(&path, e))? {
                staged.insert(rel.to_ascii_lowercase());
                stage.push(StageEntry {
                    rel: rel.clone(),
                    sha1,
                    row: None,
                });
            }
        }
        let project_id = (!f.project_id.is_empty()).then(|| f.project_id.clone());
        create.push(txn::plan_create(root, rows, rel, f.sha1.clone(), project_id, &staged).await?);
    }

    Ok(Plan {
        kind: TxnKind::PackUpdate {
            pack: a.old_origin.project_name.clone(),
            from: a.old_origin.version.clone(),
            to: a.summary.version.clone(),
        },
        stage,
        create,
        prior_pack_origin: Some(a.old_origin.clone()),
        prior_instance: Some(prior_instance),
    })
}

/// Where an old pack file is on disk, if anywhere: a mod by its registry row
/// (whose spelling says enabled or not), else whichever spelling exists; any
/// other file at its install path. Absent = nothing to set aside — it is
/// already gone.
async fn stage_entry_for(
    root: &Path,
    rows: &[InstalledMod],
    f: &PackOriginFile,
) -> Result<Option<StageEntry>, Error> {
    if f.install_path.starts_with("mods/") {
        if let Some(row) = rows.iter().find(|m| m.sha1.eq_ignore_ascii_case(&f.sha1)) {
            return Ok(Some(StageEntry {
                rel: format!("mods/{}", installed::on_disk_name(row)),
                sha1: row.sha1.to_ascii_lowercase(),
                row: Some(row.clone()),
            }));
        }
        for name in [f.filename.clone(), format!("{}.disabled", f.filename)] {
            let rel = format!("mods/{name}");
            let path = txn::content_path(root, &rel);
            if fs::try_exists(&path).await.map_err(|e| io_err(&path, e))? {
                return Ok(Some(StageEntry {
                    rel,
                    sha1: f.sha1.to_ascii_lowercase(),
                    row: None,
                }));
            }
        }
        return Ok(None);
    }
    // A path that was never safe never landed: there is nothing to set aside.
    if !is_safe_relative_path(&f.install_path) {
        return Ok(None);
    }
    let path = txn::content_path(root, &f.install_path);
    if fs::try_exists(&path).await.map_err(|e| io_err(&path, e))? {
        Ok(Some(StageEntry {
            rel: f.install_path.clone(),
            sha1: f.sha1.to_ascii_lowercase(),
            row: None,
        }))
    } else {
        Ok(None)
    }
}

fn file_name_of(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

fn io_err(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::{InstanceFile, LoaderKind};
    use crate::mods::modpack::schema::{EnvSupport, ModpackFormat, ModpackUpdateEntry};
    use crate::mods::platform::ModSource;
    use sha1::{Digest, Sha1};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Fx {
        server: MockServer,
        data: TempDir,
        inst: TempDir,
        _seam: crate::test_seam::SeamScope,
    }

    impl Fx {
        async fn new(files: &[(&str, &[u8])]) -> Self {
            let server = MockServer::start().await;
            for (name, body) in files {
                Mock::given(method("GET"))
                    .and(path(format!("/{name}")))
                    .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
                    .mount(&server)
                    .await;
            }
            let fx = Fx {
                server,
                data: TempDir::new().unwrap(),
                inst: TempDir::new().unwrap(),
                _seam: crate::test_seam::scope(&[(
                    "LUCERNA_EXTRA_ALLOWED_HOSTS",
                    "127.0.0.1, localhost",
                )]),
            };
            crate::instances::store::write_instance_json(
                &fx.root().join("instance.json"),
                &instance(),
            )
            .unwrap();
            fx
        }

        fn root(&self) -> &Path {
            self.inst.path()
        }

        /// A pack file served by this fixture, installed at `install_path`.
        fn file(&self, name: &str, install_path: &str, body: &[u8]) -> ModpackFile {
            ModpackFile {
                project_id: format!("proj-{name}"),
                version_id: format!("ver-{name}"),
                name: name.into(),
                filename: name.into(),
                install_path: install_path.into(),
                sha1: hex::encode(Sha1::digest(body)),
                md5: None,
                url: format!("{}/{name}", self.server.uri()),
                size: body.len() as f64,
                env_client: EnvSupport::Required,
                source: ModSource::Modrinth,
            }
        }

        /// Install `f` the way a pack import would; returns its origin entry.
        async fn installed(&self, f: &ModpackFile) -> PackOriginFile {
            let mv = import::modpack_file_to_mod_version(f, "1.20.1", LoaderKind::Fabric);
            install::install_one(self.data.path(), self.root(), mv, None, &nop())
                .await
                .unwrap();
            origin_file(f)
        }
    }

    fn nop() -> ProgressFn {
        Box::new(|_, _, _| {})
    }

    fn origin_file(f: &ModpackFile) -> PackOriginFile {
        PackOriginFile {
            sha1: f.sha1.clone(),
            name: f.name.clone(),
            filename: f.filename.clone(),
            install_path: f.install_path.clone(),
            url: f.url.clone(),
            size: f.size,
            project_id: f.project_id.clone(),
            version_id: f.version_id.clone(),
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        }
    }

    fn origin(version: &str, files: Vec<PackOriginFile>) -> PackOrigin {
        PackOrigin {
            project_id: Some("pack".into()),
            source: ModSource::Modrinth,
            project_name: "Pack".into(),
            version: version.into(),
            files,
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: vec![],
            inert_loader_jars: vec![],
        }
    }

    fn summary(version: &str, files: Vec<ModpackFile>) -> ModpackSummary {
        ModpackSummary {
            format: ModpackFormat::Modrinth,
            name: "Pack".into(),
            version: version.into(),
            game_version: "1.20.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            files,
            unresolvable: vec![],
            has_overrides: false,
            has_client_overrides: false,
            has_saves_in_overrides: false,
        }
    }

    fn diff(
        added: Vec<ModpackFile>,
        removed: Vec<PackOriginFile>,
        updated: Vec<ModpackUpdateEntry>,
    ) -> ModpackUpdateDiff {
        ModpackUpdateDiff {
            added,
            removed,
            updated,
            version_bump: None,
            new_version_number: "2.0".into(),
        }
    }

    fn instance() -> InstanceFile {
        InstanceFile {
            id: "pack".into(),
            uid: None,
            name: "Pack".into(),
            mc_version: "1.20.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 4096,
            min_heap_mb: None,
            extra_jvm_args: String::new(),
            created_unix_ms: 0.0,
            mrpack_name: Some("Pack".into()),
            mrpack_version: Some("1.0".into()),
            mrpack_project_id: Some("pack".into()),
            mrpack_source: Some(ModSource::Modrinth),
            mrpack_summary: None,
            mrpack_version_id: Some("old-id".into()),
            integrity: None,
            imported_from: None,
            created_from_server: None,
            handled_log_sig: None,
        }
    }

    async fn apply(
        fx: &Fx,
        d: &ModpackUpdateDiff,
        s: &ModpackSummary,
        o: &PackOrigin,
        carry: &[String],
    ) -> Result<AppliedUpdate, Error> {
        apply_update_txn(
            PackApply {
                data_dir: fx.data.path(),
                inst_root: fx.root(),
                diff: d,
                summary: s,
                old_origin: o,
                carry_disabled: carry,
                new_version_id: "new-id",
            },
            &nop(),
        )
        .await
    }

    fn mods(fx: &Fx) -> std::path::PathBuf {
        installed::mods_dir(fx.root())
    }

    fn txn_left(fx: &Fx) -> Vec<String> {
        match std::fs::read_dir(txn::txn_root(fx.root())) {
            Ok(rd) => rd
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => panic!("{e}"),
        }
    }

    #[tokio::test]
    async fn a_failure_on_the_third_file_leaves_the_profile_as_it_was() {
        let fx = Fx::new(&[
            ("old.jar", b"old"),
            ("a.jar", b"a"),
            ("b.jar", b"b"),
            ("c.jar", b"c"),
        ])
        .await;
        let old = fx
            .installed(&fx.file("old.jar", "mods/old.jar", b"old"))
            .await;
        let o = origin("1.0", vec![old.clone()]);
        installed::set_pack_origin(fx.root(), o.clone())
            .await
            .unwrap();
        // Another file already holds the third new file's name.
        std::fs::write(mods(&fx).join("c.jar"), b"users-own").unwrap();
        let new = vec![
            fx.file("a.jar", "mods/a.jar", b"a"),
            fx.file("b.jar", "mods/b.jar", b"b"),
            fx.file("c.jar", "mods/c.jar", b"c"),
        ];
        let d = diff(new.clone(), vec![old.clone()], vec![]);

        let r = apply(&fx, &d, &summary("2.0", new), &o, &[]).await;

        match r {
            Err(Error::ModpackUpdateRolledBack { file_name, cause }) => {
                assert_eq!(file_name.as_deref(), Some("c.jar"));
                // The cause stays typed, for the UI to word in the user's language.
                assert!(
                    matches!(*cause, Error::ModsFilenameConflict { ref filename, .. } if filename == "c.jar"),
                    "{cause:?}"
                );
            }
            Err(other) => panic!("expected ModpackUpdateRolledBack, got {other:?}"),
            Ok(_) => panic!("expected the update to fail"),
        }
        assert!(mods(&fx).join("old.jar").exists(), "the old file is back");
        assert!(!mods(&fx).join("a.jar").exists());
        assert!(!mods(&fx).join("b.jar").exists());
        assert_eq!(
            std::fs::read(mods(&fx).join("c.jar")).unwrap(),
            b"users-own"
        );
        let rows = installed::list(fx.root()).await.unwrap();
        assert!(rows.iter().any(|m| m.sha1 == old.sha1), "its row is back");
        assert!(!rows.iter().any(|m| m.filename == "a.jar"));
        let origin_now = installed::get_pack_origin(fx.root())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(origin_now.version, "1.0");
        let inst =
            crate::instances::store::read_instance_json(&fx.root().join("instance.json")).unwrap();
        assert_eq!(inst.mrpack_version.as_deref(), Some("1.0"));
        assert_eq!(inst.mrpack_version_id.as_deref(), Some("old-id"));
        assert!(txn_left(&fx).is_empty(), "{:?}", txn_left(&fx));
    }

    #[tokio::test]
    async fn a_pack_asset_over_the_users_file_comes_back_after_a_rollback() {
        let fx = Fx::new(&[("x.toml", b"pack")]).await;
        let o = origin("1.0", vec![]);
        installed::set_pack_origin(fx.root(), o.clone())
            .await
            .unwrap();
        let cfg = fx.root().join(".minecraft").join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("x.toml"), b"user").unwrap();
        let asset = fx.file("x.toml", "config/x.toml", b"pack");
        let mut bad = fx.file("bad.jar", "mods/bad.jar", b"bad");
        bad.url = "https://not-on-allowlist.example.invalid/bad.jar".into();
        let new = vec![asset, bad];
        let d = diff(new.clone(), vec![], vec![]);

        let r = apply(&fx, &d, &summary("2.0", new), &o, &[]).await;

        assert!(
            matches!(r, Err(Error::ModpackUpdateRolledBack { .. })),
            "{:?}",
            r.err()
        );
        assert_eq!(std::fs::read(cfg.join("x.toml")).unwrap(), b"user");
    }

    #[tokio::test]
    async fn a_successful_update_matches_the_old_outcome() {
        let fx = Fx::new(&[
            ("old.jar", b"old"),
            ("u1.jar", b"u1"),
            ("u2.jar", b"u2"),
            ("a.jar", b"a"),
        ])
        .await;
        let old = fx
            .installed(&fx.file("old.jar", "mods/old.jar", b"old"))
            .await;
        let u1 = fx.installed(&fx.file("u1.jar", "mods/u1.jar", b"u1")).await;
        install::disable(fx.root(), &u1.sha1).await.unwrap();
        let o = origin("1.0", vec![old.clone(), u1.clone()]);
        installed::set_pack_origin(fx.root(), o.clone())
            .await
            .unwrap();
        let u2 = fx.file("u2.jar", "mods/u2.jar", b"u2");
        let a = fx.file("a.jar", "mods/a.jar", b"a");
        let d = diff(
            vec![a.clone()],
            vec![old.clone()],
            vec![ModpackUpdateEntry {
                old: u1.clone(),
                new: u2.clone(),
            }],
        );
        let carry = vec![u2.sha1.clone()];

        let applied = apply(&fx, &d, &summary("2.0", vec![a, u2.clone()]), &o, &carry)
            .await
            .unwrap();

        assert_eq!(applied.details.len(), 2, "one row per placed file");
        assert!(!mods(&fx).join("old.jar").exists());
        assert!(!mods(&fx).join("u1.jar.disabled").exists());
        assert!(mods(&fx).join("a.jar").exists());
        assert!(
            mods(&fx).join("u2.jar.disabled").exists(),
            "a mod the user switched off stays off"
        );
        let origin_now = installed::get_pack_origin(fx.root())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(origin_now.version, "2.0");
        let inst =
            crate::instances::store::read_instance_json(&fx.root().join("instance.json")).unwrap();
        assert_eq!(inst.mrpack_version.as_deref(), Some("2.0"));
        assert_eq!(inst.mrpack_version_id.as_deref(), Some("new-id"));
        assert!(txn_left(&fx).is_empty(), "{:?}", txn_left(&fx));
    }
}
