//! Integrity scan. Hashes on-disk files in parallel, classifies each planned
//! artefact, and aggregates a `VerifyReport`.
//!
//! The hashing pass itself never writes. The one caveat is the manifest:
//! `verify_instance_report` calls `ensure_version_json`, which on a COLD cache
//! fetches (and writes) the version JSON — and for synth loaders runs the merge
//! / install-pipeline. For an already-installed instance (the normal case) this
//! is the disk fast-path with no network and no writes. So "read-only" holds in
//! practice but is not an absolute guarantee on a never-installed instance.

use crate::verify::ArtifactStatus;

/// Pure classification. `on_disk_sha` is `None` when the file is absent or
/// unreadable. Empty `expected_sha` ⇒ presence-only (can't be Corrupt).
/// A file that exists but is unreadable (permissions/lock) hashes to None and is reported Missing (so repair re-downloads) — intentional conflation.
pub fn classify(exists: bool, on_disk_sha: Option<&str>, expected_sha: &str) -> ArtifactStatus {
    if !exists || on_disk_sha.is_none() {
        return ArtifactStatus::Missing;
    }
    if expected_sha.is_empty() {
        return ArtifactStatus::Ok; // presence-only
    }
    if on_disk_sha == Some(expected_sha) {
        ArtifactStatus::Ok
    } else {
        ArtifactStatus::Corrupt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::ArtifactStatus;

    #[test]
    fn missing_when_absent() {
        assert_eq!(classify(false, None, "aa"), ArtifactStatus::Missing);
    }

    #[test]
    fn ok_when_hash_matches() {
        assert_eq!(classify(true, Some("aa"), "aa"), ArtifactStatus::Ok);
    }

    #[test]
    fn corrupt_when_hash_differs() {
        assert_eq!(classify(true, Some("bb"), "aa"), ArtifactStatus::Corrupt);
    }

    #[test]
    fn presence_only_ok_when_present_and_no_expected_sha() {
        assert_eq!(classify(true, Some("anything"), ""), ArtifactStatus::Ok);
    }

    #[test]
    fn presence_only_missing_when_absent_and_no_expected_sha() {
        assert_eq!(classify(false, None, ""), ArtifactStatus::Missing);
    }

    /// Deterministic non-repeating filler, so a chunk read in the wrong order,
    /// a dropped tail, or a re-hashed chunk all change the digest. A constant
    /// byte would hide every one of those.
    fn filler(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * 31 + 7) % 251) as u8).collect()
    }

    fn whole_file_sha1(bytes: &[u8]) -> String {
        use sha1::{Digest, Sha1};
        hex::encode(Sha1::digest(bytes))
    }

    /// The equivalence lock. Every size that can expose a chunk-loop bug:
    /// empty, one byte, one byte short of a chunk, exactly a chunk, one past a
    /// chunk, and several chunks with an odd remainder.
    #[tokio::test]
    async fn the_streamed_hash_equals_the_whole_file_hash_at_every_chunk_boundary() {
        let td = tempfile::tempdir().unwrap();
        let sizes = [
            0usize,
            1,
            super::HASH_CHUNK_BYTES - 1,
            super::HASH_CHUNK_BYTES,
            super::HASH_CHUNK_BYTES + 1,
            super::HASH_CHUNK_BYTES * 3 + 7,
        ];
        for size in sizes {
            let bytes = filler(size);
            let path = td.path().join(format!("artefact-{size}.bin"));
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(
                super::file_sha1(&path).await,
                Some(whole_file_sha1(&bytes)),
                "streamed digest must equal the whole-file digest at size {size}"
            );
        }
    }

    /// The conflation `classify`'s own doc states, preserved verbatim: absent
    /// and unreadable both hash to `None`, which reads as `Missing` so repair
    /// re-downloads. A directory covers the unreadable side portably — on
    /// Windows the open fails, on Unix the open succeeds and the read fails,
    /// and both must answer `None` rather than panic or hang.
    #[tokio::test]
    async fn an_absent_or_unreadable_path_still_hashes_to_none() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(super::file_sha1(&td.path().join("absent.bin")).await, None);
        assert_eq!(
            super::file_sha1(td.path()).await,
            None,
            "a directory is not an artefact"
        );
    }

    /// Peak memory must not scale with the artefact. There is no portable way to
    /// assert allocation from a unit test, so this asserts the one thing that
    /// IS observable and load-bearing: the buffer is a compile-time constant and
    /// a sane one. The real guarantee lives in
    /// `tests/structural_verify_hash_offloaded.rs`.
    #[test]
    fn the_hash_buffer_is_a_fixed_size_not_a_function_of_the_file() {
        assert!(super::HASH_CHUNK_BYTES >= 64 * 1024);
        assert!(super::HASH_CHUNK_BYTES <= 1024 * 1024);
    }

    #[test]
    fn manifest_recoverable_flag_makes_report_unhealthy() {
        // Guards FIX 2: a recoverable-manifest report must not be healthy even
        // with no per-file problems.
        let r = crate::verify::VerifyReport::build(
            "i".into(),
            "1.20.4".into(),
            &[(crate::verify::VerifyCategory::Assets, 0)],
            vec![],
            true,
        );
        assert!(!r.healthy);
    }
}

use crate::error::{Error, Result};
use crate::verify::plan::{asset_artifact, client_artifact, library_artifacts, PlannedArtifact};
use crate::verify::progress::{VerifyPhase, VerifyProgress};
use crate::verify::{ProblemArtifact, VerifyCategory, VerifyReport};
use crate::versions::install::ensure_version_json;
use futures_util::stream::{self, StreamExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tauri_specta::Event;

const CONCURRENCY: usize = 8;

/// Chunk size for the streamed hash. Large enough that the per-chunk syscall
/// and `Sha1::update` overhead is noise; small enough that eight concurrent
/// hashes hold about a megabyte of buffers between them instead of eight whole
/// artefacts. Mirrors the 64 KiB streaming loop in
/// `worlds::zip::extract_zip_capped`, doubled because nothing here needs a
/// per-chunk cap check.
const HASH_CHUNK_BYTES: usize = 128 * 1024;

/// An artefact resolved to its absolute on-disk path, ready to hash.
pub struct PlannedOnDisk {
    pub abs_path: PathBuf,
    pub expected_sha: String,
}

/// SHA-1 of a file, streamed in [`HASH_CHUNK_BYTES`] chunks on a blocking
/// thread. `None` when the file is absent or unreadable — [`classify`] reports
/// that as `Missing` so repair re-downloads it, the intentional conflation its
/// own doc comment already states. That contract is unchanged here.
///
/// Two properties matter, and neither is cosmetic:
///
/// * **`spawn_blocking`.** Reading and SHA-1'ing is CPU plus blocking IO. Run
///   directly on the async executor, [`hash_planned`]'s `buffer_unordered(8)`
///   pins up to eight tokio WORKER threads for the length of a full verify and
///   starves every other task on the runtime — the same reasoning
///   `tests/structural_no_heavy_sync_command.rs` spells out for the main
///   thread, applied one level down to the worker pool. `spawn_blocking` moves
///   it to the blocking pool, which exists for exactly this.
/// * **Streaming.** `tokio::fs::read` materialised the whole artefact in memory,
///   so peak usage was eight times the largest one at once. A fixed chunk buffer
///   makes peak usage independent of artefact size.
///
/// A panic inside the blocking task surfaces as a `JoinError`. It is logged and
/// folded into `None` rather than unwrapped: verify is a diagnostic pass, and
/// reporting one artefact `Missing` is a far better failure than aborting the
/// whole report.
async fn file_sha1(path: &std::path::Path) -> Option<String> {
    let owned = path.to_path_buf();
    match tokio::task::spawn_blocking(move || sha1_of_file(&owned)).await {
        Ok(result) => result,
        Err(e) => {
            crate::diag!("verify: hash task for {} did not join: {e}", path.display());
            None
        }
    }
}

/// Blocking half of [`file_sha1`]: open, read in fixed-size chunks, feed each
/// chunk to the digest. Never allocates in proportion to the file.
///
/// The incremental `Digest` API (`new` / `update` / `finalize`) is the same one
/// `accounts::microsoft::oauth::Pkce::new` already uses with `Sha256`; both
/// crates sit on the same `digest` release in this tree.
fn sha1_of_file(path: &std::path::Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        // A mid-read failure yields `None`, i.e. `Missing` — identical to the
        // whole-file read this replaces, and to the conflation `classify`
        // documents. Not a new fallback: the same one, in chunks.
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hex::encode(hasher.finalize()))
}

/// Hash each item in parallel and classify. `on_progress(done, total, bytes)`
/// fires after each file (bytes currently always 0 — progress is file-count
/// based). Preserves input order in the returned vec.
///
/// `concurrency` bounds how many hashes are in flight, which since [`file_sha1`]
/// moved to `spawn_blocking` means how many BLOCKING-pool threads this pass
/// occupies — not how many async workers it starves. The bound is kept as-is:
/// it is what stops a verify from saturating the blocking pool and stalling
/// every other `spawn_blocking` caller in the app.
pub async fn hash_planned(
    items: Vec<PlannedOnDisk>,
    concurrency: usize,
    on_progress: impl Fn(u32, u32, u64) + Send + Sync + 'static,
) -> Vec<ArtifactStatus> {
    let total = items.len() as u32;
    let progress = Arc::new(on_progress);
    let done = Arc::new(AtomicU32::new(0));

    let mut indexed: Vec<(usize, ArtifactStatus)> = stream::iter(items.into_iter().enumerate())
        .map(|(i, item)| {
            let progress = Arc::clone(&progress);
            let done = Arc::clone(&done);
            async move {
                let exists = tokio::fs::metadata(&item.abs_path).await.is_ok();
                let on_disk = if exists {
                    file_sha1(&item.abs_path).await
                } else {
                    None
                };
                let status = classify(exists, on_disk.as_deref(), &item.expected_sha);
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(d, total, 0);
                (i, status)
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect::<Vec<_>>()
        .await;

    indexed.sort_by_key(|(i, _)| *i);
    indexed.into_iter().map(|(_, s)| s).collect()
}

/// Full read-only verification for an instance's effective version. Offline
/// for the hashing pass; only the network if ensure_version_json must fetch.
pub async fn verify_instance_report(
    instance_id: &str,
    effective_id: &str,
    app: &tauri::AppHandle,
) -> Result<VerifyReport> {
    emit(app, instance_id, VerifyPhase::Manifest, 0, 1, None);

    // Profile JSON / manifest. On failure the manifest is unrecoverable.
    let details = match ensure_version_json(effective_id, app).await {
        Ok(d) => d,
        Err(_) => {
            return Ok(VerifyReport::build(
                instance_id.to_string(),
                effective_id.to_string(),
                &[(VerifyCategory::ProfileJson, 1)],
                vec![ProblemArtifact {
                    category: VerifyCategory::ProfileJson,
                    rel_path: format!("{effective_id}.json"),
                    expected_sha: String::new(),
                    url: None,
                    status: ArtifactStatus::Missing,
                }],
                true,
            ));
        }
    };

    let os = crate::versions::install::current_os();
    let arch = crate::versions::install::current_arch();

    let versions = crate::paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let libraries_dir =
        crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries_dir>", e))?;
    let assets_root = crate::paths::assets_dir(app).map_err(|e| Error::io("<assets_dir>", e))?;
    let assets_objects = assets_root.join("objects");

    let mut planned: Vec<(PlannedArtifact, PathBuf)> = Vec::new();

    // Client jar. Normally `downloads` is Some post-merge (vanilla parent
    // supplies it). If it's absent (defensive), still PRESENCE-check the jar
    // at versions/<parent_mc>/<parent_mc>.jar so a missing jar isn't silently
    // reported healthy — we just can't detect corruption without a SHA.
    let client = match details.downloads.as_ref() {
        Some(downloads) => {
            client_artifact(effective_id, &downloads.client.sha1, &downloads.client.url)
        }
        None => client_artifact(effective_id, "", ""),
    };
    let client_abs = versions.join(&client.rel_path);
    planned.push((client, client_abs));

    for lib in library_artifacts(&details, os, arch) {
        let abs = libraries_dir.join(&lib.rel_path);
        planned.push((lib, abs));
    }

    let mut manifest_recoverable = false;
    if let Some(ai) = details.asset_index.as_ref() {
        let index_file = assets_root.join("indexes").join(format!("{}.json", ai.id));
        match tokio::fs::read(&index_file).await {
            Ok(raw) => match serde_json::from_slice::<crate::versions::assets::AssetIndex>(&raw) {
                Ok(parsed) => {
                    for obj in parsed.objects.values() {
                        let a = asset_artifact(&obj.hash);
                        let abs = assets_objects.join(&a.rel_path);
                        planned.push((a, abs));
                    }
                }
                Err(e) => {
                    // Unparseable index: we can't enumerate objects, so the
                    // assets can't be verified. Mark recoverable so the report
                    // is unhealthy and repair re-fetches the index via install.
                    crate::diag!("verify: asset index {} unparseable: {e}", ai.id);
                    manifest_recoverable = true;
                }
            },
            Err(e) => {
                crate::diag!("verify: asset index {} unreadable: {e}", ai.id);
                manifest_recoverable = true;
            }
        }
    }

    let on_disk: Vec<PlannedOnDisk> = planned
        .iter()
        .map(|(p, abs)| PlannedOnDisk {
            abs_path: abs.clone(),
            expected_sha: p.expected_sha.clone(),
        })
        .collect();

    let app_clone = app.clone();
    let id_owned = instance_id.to_string();
    let statuses = hash_planned(on_disk, CONCURRENCY, move |done, total, _bytes| {
        emit(
            &app_clone,
            &id_owned,
            VerifyPhase::Hashing,
            done,
            total,
            None,
        );
    })
    .await;

    let jre_component = details
        .java_version
        .as_ref()
        .map(|jv| jv.component.clone())
        .unwrap_or_else(|| crate::jre::DEFAULT_LEGACY_COMPONENT.to_string());
    let jre_ok = jre_present(&jre_component, app).await;

    let mut problems: Vec<ProblemArtifact> = Vec::new();
    for ((p, _abs), status) in planned.iter().zip(statuses.iter()) {
        if *status != ArtifactStatus::Ok {
            problems.push(ProblemArtifact {
                category: p.category,
                rel_path: p.rel_path.clone(),
                expected_sha: p.expected_sha.clone(),
                url: p.url.clone(),
                status: *status,
            });
        }
    }
    if !jre_ok {
        problems.push(ProblemArtifact {
            category: VerifyCategory::Jre,
            rel_path: format!("{jre_component}/bin"),
            expected_sha: String::new(),
            url: None,
            status: ArtifactStatus::Missing,
        });
    }

    let totals = category_totals(&planned, 1);
    let report = VerifyReport::build(
        instance_id.to_string(),
        effective_id.to_string(),
        &totals,
        problems,
        manifest_recoverable,
    );

    emit(app, instance_id, VerifyPhase::Complete, 1, 1, None);
    Ok(report)
}

/// Marker present + parseable + the java executable exists.
async fn jre_present(component: &str, app: &tauri::AppHandle) -> bool {
    let Ok(marker) = crate::jre::install::marker_path(component, app) else {
        return false;
    };
    let marker_ok = tokio::fs::read_to_string(&marker)
        .await
        .ok()
        .and_then(|s| crate::jre::install::Marker::parse(&s))
        .is_some();
    if !marker_ok {
        return false;
    }
    match crate::jre::java_executable_path(component, app) {
        Ok(exe) => tokio::fs::metadata(&exe).await.is_ok(),
        Err(_) => false,
    }
}

fn category_totals(
    planned: &[(PlannedArtifact, PathBuf)],
    jre_total: u32,
) -> Vec<(VerifyCategory, u32)> {
    use VerifyCategory::*;
    let count = |c: VerifyCategory| planned.iter().filter(|(p, _)| p.category == c).count() as u32;
    vec![
        (Client, count(Client)),
        (Libraries, count(Libraries)),
        (Assets, count(Assets)),
        (Jre, jre_total),
        // ProfileJson is one artefact, total always 1. It is presence/parse-checked
        // only — it has no authoritative SHA — so a JSON that parses but is
        // semantically wrong reads as OK here; a missing/unparseable one is caught
        // earlier via the manifest_recoverable early-return (so it never reaches here
        // showing green). The 1/1 OK in the happy path means "parsed", not "SHA-verified".
        (ProfileJson, 1),
    ]
}

fn emit(
    app: &tauri::AppHandle,
    instance_id: &str,
    phase: VerifyPhase,
    files_done: u32,
    files_total: u32,
    current_category: Option<VerifyCategory>,
) {
    // Best-effort: a dropped event (no listener / closed window) is fine.
    let _ = VerifyProgress {
        instance_id: instance_id.to_string(),
        phase,
        files_done,
        files_total,
        bytes_done: 0.0,
        current_category,
    }
    .emit(app);
}
