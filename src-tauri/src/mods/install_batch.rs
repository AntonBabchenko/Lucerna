//! Atomic multi-mod install. The batch counterpart of `update_one`'s
//! "warm the cache, then swap" shape:
//!
//! - Phase 0 — guards (safe filename, distribution allowed, SHA-1 present)
//!   over the WHOLE sequence, before any network or instance I/O.
//! - Phase 1 — warm the shared content cache; a failure here (the dominant
//!   class: network) leaves the instance completely untouched.
//! - Phase 2 — commit each item from the warm cache (`install_one`). On a
//!   failure, best-effort rollback: newly-created files and newly-added
//!   registry records of every attempted item are removed; pre-existing
//!   files/records are never touched. `reconcile()` self-heals any residue.
//!
//! The two pre-run snapshots (mods/ file names, registry SHA-1s) are read
//! independently and can in principle disagree if a concurrent mutation lands
//! between the reads — the rollback halves then act on slightly different
//! truths. That window is milliseconds wide and any resulting orphan record
//! is dropped by the registry's next reconciling `list()`, so it is accepted
//! rather than locked against.

use std::collections::HashSet;
use std::path::Path;

use crate::error::Error;
use crate::mods::install::{self, Installed, ProgressFn};
use crate::mods::installed;
use crate::mods::platform::ModVersion;

/// Which sequence member failed and why. The caller emits its
/// `mod-install-failed` event from `project_id` and surfaces `error`.
#[derive(Debug)]
pub struct BatchFailure {
    pub project_id: String,
    pub error: Error,
}

/// Install `items` in order, atomically. On success returns the per-item
/// outcomes in `items` order — the caller emits its per-mod events from them
/// AFTER the whole batch has committed, so a rollback can never contradict an
/// already-sent success event.
pub async fn install_batch(
    data_dir: &Path,
    instance_root: &Path,
    items: &[ModVersion],
    progress: &ProgressFn,
) -> Result<Vec<Installed>, BatchFailure> {
    // Phase 0 — guards over the whole sequence before any I/O.
    let mut shas: Vec<String> = Vec::with_capacity(items.len());
    for v in items {
        match install::guard_version(v) {
            Ok(sha) => shas.push(sha),
            Err(error) => {
                return Err(BatchFailure {
                    project_id: v.project_id.clone(),
                    error,
                })
            }
        }
    }

    // Phase 1 — warm the cache; the instance is untouched on failure.
    for (v, sha) in items.iter().zip(shas.iter()) {
        if let Err(error) = install::fetch_to_cache(
            data_dir,
            &v.primary_file.url,
            sha,
            v.primary_file.size,
            "mods",
            progress,
        )
        .await
        {
            return Err(BatchFailure {
                project_id: v.project_id.clone(),
                error,
            });
        }
    }

    // Phase 2 — commit from the warm cache. Snapshot the pre-run state first
    // so rollback can distinguish what this run created. `None` = snapshot
    // unreadable → rollback stays conservative for that half.
    let pre_files = pre_existing_files(instance_root).await;
    if pre_files.is_none() {
        crate::diag!("install_batch: mods/ unreadable — a rollback would skip file removals");
    }
    let pre_shas = pre_existing_shas(instance_root).await;
    if pre_shas.is_none() {
        crate::diag!("install_batch: registry unreadable — a rollback would skip record removals");
    }
    let mut done: Vec<Installed> = Vec::new();
    for (i, v) in items.iter().enumerate() {
        match install::install_one(data_dir, instance_root, v.clone(), progress).await {
            Ok(inst) => done.push(inst),
            Err(error) => {
                rollback(instance_root, &items[..=i], &pre_files, &pre_shas).await;
                return Err(BatchFailure {
                    project_id: v.project_id.clone(),
                    error,
                });
            }
        }
    }
    Ok(done)
}

/// Lowercased file names currently in `mods/`. Lowercased because the
/// filesystems this launcher primarily targets (Windows, default macOS) are
/// case-insensitive: a candidate `sodium.jar` collides with an on-disk
/// `Sodium.jar` there, so the rollback's pre-existing protection must compare
/// names the way the filesystem does. `Some(empty)` for a missing dir (fresh
/// instance), `None` when the dir exists but cannot be read — the rollback
/// then skips file removals rather than risk deleting pre-existing jars.
async fn pre_existing_files(instance_root: &Path) -> Option<HashSet<String>> {
    let dir = installed::mods_dir(instance_root);
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(HashSet::new()),
        Err(_) => return None,
    };
    let mut out = HashSet::new();
    loop {
        match rd.next_entry().await {
            Ok(Some(e)) => {
                out.insert(e.file_name().to_string_lossy().to_ascii_lowercase());
            }
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    Some(out)
}

/// Lowercased SHA-1s currently in the registry (raw read, no reconcile).
/// `None` when the registry cannot be read — rollback then skips record
/// removals rather than risk dropping pre-existing records.
async fn pre_existing_shas(instance_root: &Path) -> Option<HashSet<String>> {
    match installed::read_or_empty(instance_root).await {
        Ok(state) => Some(
            state
                .mods
                .iter()
                .map(|m| m.sha1.to_ascii_lowercase())
                .collect(),
        ),
        Err(_) => None,
    }
}

/// Best-effort undo of `attempted` (committed items plus the failed one).
/// Never masks the original error — every failure here is only logged.
///
/// Since `install_one` materializes through [`crate::mods::store`] (temp name
/// then rename, cleaned up on every failure path), a failed item can no longer
/// leave a partial file at its destination — the destination is either fully
/// replaced or exactly as it was. Removing an instance's own file is also safe
/// when that file is a hardlink shared with another instance: deleting one name
/// never touches the bytes the other names still hold.
async fn rollback(
    instance_root: &Path,
    attempted: &[ModVersion],
    pre_files: &Option<HashSet<String>>,
    pre_shas: &Option<HashSet<String>>,
) {
    let dir = installed::mods_dir(instance_root);
    if let Some(pre_files) = pre_files {
        for v in attempted {
            let filename = &v.primary_file.filename;
            // Case-insensitive on purpose — see `pre_existing_files`.
            if pre_files.contains(&filename.to_ascii_lowercase()) {
                continue; // pre-existing file (idempotent reinstall) — keep
            }
            let p = dir.join(filename);
            match tokio::fs::remove_file(&p).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    crate::diag!(
                        "install_batch: rollback failed to remove {}: {e}",
                        p.display()
                    );
                }
            }
        }
    }
    if let Some(pre_shas) = pre_shas {
        let added: HashSet<String> = attempted
            .iter()
            .filter_map(|v| v.primary_file.sha1.as_deref())
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && !pre_shas.contains(s))
            .collect();
        if let Err(e) = installed::remove_many(instance_root, &added).await {
            crate::diag!("install_batch: rollback failed to deregister records: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::cache;
    use crate::mods::platform::{LoaderKind, ModFile, ModSource};
    use sha1::{Digest, Sha1};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn nop_progress() -> ProgressFn {
        Box::new(|_, _, _| {})
    }

    fn fake_version(url: String, sha: String, len: u64, filename: &str, pid: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: pid.into(),
            version_id: format!("{pid}-v"),
            name: format!("Mod {pid}"),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: filename.into(),
                url,
                sha1: Some(sha),
                size: len as f64,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    async fn mock_jar(s: &MockServer, route: &str, body: &[u8]) -> String {
        Mock::given(method("GET"))
            .and(path(route.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(s)
            .await;
        hex::encode(Sha1::digest(body))
    }

    #[tokio::test]
    async fn warm_phase_failure_leaves_instance_untouched() {
        let s = MockServer::start().await;
        let sha_ok = mock_jar(&s, "/ok.jar", b"ok-bytes").await;
        // No mock for /missing.jar → 404 during the warm phase.
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let items = vec![
            fake_version(format!("{}/ok.jar", s.uri()), sha_ok, 8, "ok.jar", "p-ok"),
            fake_version(
                format!("{}/missing.jar", s.uri()),
                "f".repeat(40),
                5,
                "missing.jar",
                "p-missing",
            ),
        ];
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let f = install_batch(td_data.path(), td_inst.path(), &items, &nop_progress())
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-missing");
        // The instance was never touched: ok.jar was fetched to the cache
        // only, never copied, never recorded.
        assert!(!installed::mods_dir(td_inst.path()).exists());
        assert!(installed::list(td_inst.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn commit_phase_failure_rolls_back_committed_items() {
        let s = MockServer::start().await;
        let sha_a = mock_jar(&s, "/a.jar", b"a-bytes").await;
        let sha_b = mock_jar(&s, "/b.jar", b"b-new-bytes").await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        // Pre-place b.jar with DIFFERENT bytes: warm phase succeeds, commit of
        // item b fails with ModsFilenameConflict — after item a committed.
        let dir = installed::mods_dir(td_inst.path());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("b.jar"), b"b-preexisting")
            .await
            .unwrap();
        let items = vec![
            fake_version(
                format!("{}/a.jar", s.uri()),
                sha_a.clone(),
                7,
                "a.jar",
                "p-a",
            ),
            fake_version(format!("{}/b.jar", s.uri()), sha_b, 11, "b.jar", "p-b"),
        ];
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let f = install_batch(td_data.path(), td_inst.path(), &items, &nop_progress())
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-b");
        assert!(matches!(f.error, Error::ModsFilenameConflict { .. }));
        // Item a was rolled back: file gone, no registry record.
        assert!(!dir.join("a.jar").exists());
        // The user's pre-existing b.jar was NOT touched.
        assert_eq!(
            tokio::fs::read(dir.join("b.jar")).await.unwrap(),
            b"b-preexisting"
        );
        let listed = installed::list(td_inst.path()).await.unwrap();
        assert!(!listed.iter().any(|m| m.sha1.eq_ignore_ascii_case(&sha_a)));
    }

    #[tokio::test]
    async fn intra_batch_filename_collision_rolls_back_cleanly() {
        let s = MockServer::start().await;
        let sha1 = mock_jar(&s, "/x1.jar", b"x-first").await;
        let sha2 = mock_jar(&s, "/x2.jar", b"x-second").await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        // Two DIFFERENT files sharing one target filename: item 2's commit
        // conflicts against item 1's own freshly-written jar, and the
        // rollback must undo item 1 without tripping on the shared name.
        let items = vec![
            fake_version(
                format!("{}/x1.jar", s.uri()),
                sha1.clone(),
                7,
                "x.jar",
                "p-x1",
            ),
            fake_version(format!("{}/x2.jar", s.uri()), sha2, 8, "x.jar", "p-x2"),
        ];
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let f = install_batch(td_data.path(), td_inst.path(), &items, &nop_progress())
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-x2");
        assert!(matches!(f.error, Error::ModsFilenameConflict { .. }));
        assert!(!installed::mods_dir(td_inst.path()).join("x.jar").exists());
        assert!(installed::list(td_inst.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rollback_protects_pre_existing_files_case_insensitively() {
        // Windows/macOS default filesystems are case-insensitive: a candidate
        // named `sodium.jar` collides with a pre-existing `Sodium.jar`. The
        // pre-run snapshot must protect that file regardless of case, or the
        // rollback would delete the very file the conflict guard refused to
        // overwrite. (On a case-sensitive filesystem the remove targets a
        // different name and no-ops — the assertion holds on every platform.)
        // Both case directions: on-disk uppercase vs lowercase candidate AND
        // on-disk lowercase vs mixed-case candidate — a regression on either
        // side of the comparison (snapshot or lookup) must trip one of them.
        let td_inst = TempDir::new().unwrap();
        let dir = installed::mods_dir(td_inst.path());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("Sodium.jar"), b"pre-existing")
            .await
            .unwrap();
        tokio::fs::write(dir.join("lithium.jar"), b"pre-existing-2")
            .await
            .unwrap();
        let attempted = vec![
            fake_version(
                "http://127.0.0.1:1/sodium.jar".into(),
                "a".repeat(40),
                12,
                "sodium.jar",
                "p-sodium",
            ),
            fake_version(
                "http://127.0.0.1:1/Lithium.jar".into(),
                "b".repeat(40),
                14,
                "Lithium.jar",
                "p-lithium",
            ),
        ];
        let pre_files = pre_existing_files(td_inst.path()).await;
        let pre_shas = pre_existing_shas(td_inst.path()).await;
        rollback(td_inst.path(), &attempted, &pre_files, &pre_shas).await;
        assert_eq!(
            tokio::fs::read(dir.join("Sodium.jar")).await.unwrap(),
            b"pre-existing"
        );
        assert_eq!(
            tokio::fs::read(dir.join("lithium.jar")).await.unwrap(),
            b"pre-existing-2"
        );
    }

    #[tokio::test]
    async fn pre_existing_same_sha_item_survives_rollback() {
        let s = MockServer::start().await;
        let sha_a = mock_jar(&s, "/pre.jar", b"pre-bytes").await;
        let sha_b = mock_jar(&s, "/con.jar", b"con-new").await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // pre.jar is ALREADY properly installed before the batch runs.
        let pre = fake_version(
            format!("{}/pre.jar", s.uri()),
            sha_a.clone(),
            9,
            "pre.jar",
            "p-pre",
        );
        install::install_one(td_data.path(), td_inst.path(), pre.clone(), &nop_progress())
            .await
            .unwrap();
        // Conflict trap for the second item.
        let dir = installed::mods_dir(td_inst.path());
        tokio::fs::write(dir.join("con.jar"), b"con-preexisting")
            .await
            .unwrap();
        let items = vec![
            pre, // idempotent re-install
            fake_version(format!("{}/con.jar", s.uri()), sha_b, 7, "con.jar", "p-con"),
        ];
        let f = install_batch(td_data.path(), td_inst.path(), &items, &nop_progress())
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-con");
        // The pre-existing install survives the rollback: file AND record.
        assert!(dir.join("pre.jar").exists());
        let listed = installed::list(td_inst.path()).await.unwrap();
        assert!(listed.iter().any(|m| m.sha1.eq_ignore_ascii_case(&sha_a)));
    }

    #[tokio::test]
    async fn success_installs_all_in_order() {
        let s = MockServer::start().await;
        let sha_a = mock_jar(&s, "/s1.jar", b"s1").await;
        let sha_b = mock_jar(&s, "/s2.jar", b"s2").await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let items = vec![
            fake_version(
                format!("{}/s1.jar", s.uri()),
                sha_a.clone(),
                2,
                "s1.jar",
                "p-1",
            ),
            fake_version(
                format!("{}/s2.jar", s.uri()),
                sha_b.clone(),
                2,
                "s2.jar",
                "p-2",
            ),
        ];
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let done = install_batch(td_data.path(), td_inst.path(), &items, &nop_progress())
            .await
            .unwrap();
        assert_eq!(done.len(), 2);
        assert_eq!(done[0].sha1, sha_a);
        assert_eq!(done[1].sha1, sha_b);
        let dir = installed::mods_dir(td_inst.path());
        assert!(dir.join("s1.jar").exists());
        assert!(dir.join("s2.jar").exists());
        assert_eq!(installed::list(td_inst.path()).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn guards_run_for_whole_sequence_before_any_network() {
        let s = MockServer::start().await;
        // expect(0): if guards were per-item-interleaved, item 1 would be
        // fetched before item 2's distribution guard fired.
        Mock::given(method("GET"))
            .and(path("/g1.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"g1".to_vec()))
            .expect(0)
            .mount(&s)
            .await;
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();
        let sha1 = hex::encode(Sha1::digest(b"g1"));
        let mut blocked = fake_version(
            format!("{}/g2.jar", s.uri()),
            "a".repeat(40),
            2,
            "g2.jar",
            "p-g2",
        );
        blocked.primary_file.distribution_allowed = false;
        let items = vec![
            fake_version(
                format!("{}/g1.jar", s.uri()),
                sha1.clone(),
                2,
                "g1.jar",
                "p-g1",
            ),
            blocked,
        ];
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let f = install_batch(td_data.path(), td_inst.path(), &items, &nop_progress())
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-g2");
        assert!(matches!(f.error, Error::ModsDistributionDisabled { .. }));
        assert!(!cache::cache_path_for(td_data.path(), &sha1).exists());
    }
}
