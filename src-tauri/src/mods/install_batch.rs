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

use std::collections::HashSet;
use std::path::Path;

use crate::error::Error;
use crate::mods::install::{self, Installed, ProgressFn};
use crate::mods::installed;
use crate::mods::platform::{ModSource, ModVersion};

/// Which sequence member failed and why. The caller emits its
/// `mod-install-failed` event from `project_id` and surfaces `error`.
#[derive(Debug)]
pub struct BatchFailure {
    pub project_id: String,
    pub error: Error,
}

/// Install `items` in order, atomically. `on_installed` fires after each item
/// lands (the caller emits its per-mod event from it). On success returns the
/// per-item outcomes in `items` order.
pub async fn install_batch(
    data_dir: &Path,
    instance_root: &Path,
    items: Vec<ModVersion>,
    progress: &ProgressFn,
    on_installed: &(dyn Fn(&Installed) + Send + Sync),
) -> Result<Vec<Installed>, BatchFailure> {
    // Phase 0 — guards over the whole sequence before any I/O.
    let mut shas: Vec<String> = Vec::with_capacity(items.len());
    for v in &items {
        match guard_item(v) {
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
    let pre_shas = pre_existing_shas(instance_root).await;
    let mut done: Vec<Installed> = Vec::new();
    for (i, v) in items.iter().enumerate() {
        match install::install_one(data_dir, instance_root, v.clone(), progress).await {
            Ok(inst) => {
                on_installed(&inst);
                done.push(inst);
            }
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

/// Pre-instance-I/O checks for one item; returns the normalized (lowercased)
/// SHA-1 on success. Mirrors the guard block of `install_one` / `update_one`.
fn guard_item(v: &ModVersion) -> Result<String, Error> {
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
                ModSource::Hangar => "hangar",
            }
            .into(),
            project_id: v.project_id.clone(),
        });
    }
    match v.primary_file.sha1.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s.to_ascii_lowercase()),
        _ => Err(Error::ModsSha1Unavailable),
    }
}

/// File names currently in `mods/`. `Some(empty)` for a missing dir (fresh
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
                out.insert(e.file_name().to_string_lossy().to_string());
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

/// Best-effort undo of `attempted` (committed items plus the failed one,
/// whose `fs::copy` may have left a partial file). Never masks the original
/// error — every failure here is only logged.
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
            if pre_files.contains(filename) {
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
    use crate::mods::platform::{LoaderKind, ModFile};
    use sha1::{Digest, Sha1};
    use std::sync::Mutex;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn nop_progress() -> ProgressFn {
        Box::new(|_, _, _| {})
    }

    fn nop_on_installed() -> impl Fn(&Installed) + Send + Sync {
        |_: &Installed| {}
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
        let cb = nop_on_installed();
        let f = install_batch(td_data.path(), td_inst.path(), items, &nop_progress(), &cb)
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
        let cb = nop_on_installed();
        let f = install_batch(td_data.path(), td_inst.path(), items, &nop_progress(), &cb)
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
        let cb = nop_on_installed();
        let f = install_batch(td_data.path(), td_inst.path(), items, &nop_progress(), &cb)
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-con");
        // The pre-existing install survives the rollback: file AND record.
        assert!(dir.join("pre.jar").exists());
        let listed = installed::list(td_inst.path()).await.unwrap();
        assert!(listed.iter().any(|m| m.sha1.eq_ignore_ascii_case(&sha_a)));
    }

    #[tokio::test]
    async fn success_installs_all_in_order_and_fires_on_installed() {
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
        let seen: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let on_installed = |inst: &Installed| {
            seen.lock().unwrap().push(inst.filename.clone());
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let done = install_batch(
            td_data.path(),
            td_inst.path(),
            items,
            &nop_progress(),
            &on_installed,
        )
        .await
        .unwrap();
        assert_eq!(done.len(), 2);
        assert_eq!(done[0].sha1, sha_a);
        assert_eq!(done[1].sha1, sha_b);
        assert_eq!(*seen.lock().unwrap(), vec!["s1.jar", "s2.jar"]);
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
        let cb = nop_on_installed();
        let f = install_batch(td_data.path(), td_inst.path(), items, &nop_progress(), &cb)
            .await
            .unwrap_err();
        assert_eq!(f.project_id, "p-g2");
        assert!(matches!(f.error, Error::ModsDistributionDisabled { .. }));
        assert!(!cache::cache_path_for(td_data.path(), &sha1).exists());
    }
}
