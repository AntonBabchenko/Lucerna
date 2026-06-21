//! Hash-enrichment of modpack override-bundled mods.
//!
//! When a modpack is imported, jars bundled loose in the pack's
//! `overrides/mods/` folder land in the registry with `source = None`.
//! This module identifies them by file hash — SHA-1 against Modrinth,
//! a MurmurHash2 fingerprint against CurseForge — and backfills the
//! recovered platform identity so the Installed view can render icons.
//! Best-effort throughout: a network failure never blocks an import or
//! the Installed view, and never raises a user-facing error.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::error::Error;
use crate::instances::schema::LoaderKind;
use crate::mods::installed;
use crate::mods::platform::{InstalledMod, ModSource, ResolvedIdentity, VersionRef};

const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

/// CurseForge file fingerprint: MurmurHash2 (32-bit, seed 1) over the
/// file bytes with the whitespace bytes 0x09, 0x0a, 0x0d, 0x20 removed,
/// hashed over the post-strip length. This is CurseForge's documented
/// fingerprint algorithm — the `/v1/fingerprints` endpoint matches jars
/// by exactly this value. Implemented in-crate (~30 lines) so no new
/// crate dependency is pulled.
pub fn curseforge_fingerprint(bytes: &[u8]) -> u32 {
    let stripped: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, 0x09 | 0x0a | 0x0d | 0x20))
        .collect();
    murmur2(&stripped, 1)
}

/// MurmurHash2, 32-bit, by Austin Appleby. All multiplications wrap
/// (`u32` arithmetic mod 2^32). `data.len() as u32` truncates above 4
/// GiB — irrelevant for mod jars, which are at most a few MiB.
fn murmur2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;
    let mut h: u32 = seed ^ (data.len() as u32);
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let tail = chunks.remainder();
    if tail.len() == 3 {
        h ^= (tail[2] as u32) << 16;
    }
    if tail.len() >= 2 {
        h ^= (tail[1] as u32) << 8;
    }
    if !tail.is_empty() {
        h ^= tail[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// Choose one identity for a jar from the two per-platform lookup
/// results. When both platforms matched, the pack's home platform wins
/// — keeping a recovered identity consistent with the pack it came
/// from. With a match on only one platform, that one is used.
fn pick_identity(
    modrinth: Option<VersionRef>,
    curseforge: Option<VersionRef>,
    home: ModSource,
) -> Option<VersionRef> {
    match (modrinth, curseforge) {
        (Some(mr), Some(cf)) => Some(if home == ModSource::Curseforge {
            cf
        } else {
            mr
        }),
        (Some(mr), None) => Some(mr),
        (None, Some(cf)) => Some(cf),
        (None, None) => None,
    }
}

/// The fields the resolve path needs from a Modrinth version object;
/// serde ignores the rest of the (large) payload. `loaders` /
/// `game_versions` drive the instance-compatibility check (a byte-identical
/// jar can be attached to versions with different tags — see `MrMatch`).
#[derive(Deserialize)]
struct MrVersionLite {
    id: String,
    project_id: String,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    game_versions: Vec<String>,
}

/// A Modrinth hash match: the identity plus the matched version's loader/MC
/// tags, so the caller can detect when `version_files` returned a version
/// that does not match the instance (a shared "universal" jar) and record
/// the project without a misleading version.
#[derive(Debug, Clone)]
struct MrMatch {
    project_id: String,
    version_id: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
}

impl MrMatch {
    fn version_ref(&self) -> VersionRef {
        VersionRef {
            source: ModSource::Modrinth,
            project_id: self.project_id.clone(),
            version_id: self.version_id.clone(),
        }
    }
}

/// `true` iff a Modrinth version's tags are compatible with the instance:
/// the instance's loader slug appears in the version's `loaders` and the
/// instance's MC version appears in its `game_versions`. A multi-loader
/// "universal" version that lists the instance loader matches.
fn modrinth_version_matches_instance(
    version_loaders: &[String],
    version_game_versions: &[String],
    instance_loader: LoaderKind,
    instance_mc: &str,
) -> bool {
    let slug = instance_loader.modrinth_slug();
    let loader_ok = version_loaders.iter().any(|l| l.eq_ignore_ascii_case(slug));
    let mc_ok = version_game_versions.iter().any(|g| g == instance_mc);
    loader_ok && mc_ok
}

/// Read the instance's loader + MC version from `instance.json`, best-effort.
/// `None` on any failure (missing/corrupt file) — the caller then keeps the
/// hash-matched version verbatim, i.e. the pre-fix behavior.
async fn read_instance_loader_mc(instance_root: &Path) -> Option<(LoaderKind, String)> {
    let bytes = tokio::fs::read(instance_root.join("instance.json"))
        .await
        .ok()?;
    let inst: crate::instances::schema::InstanceFile = serde_json::from_slice(&bytes).ok()?;
    Some((inst.loader, inst.mc_version))
}

/// Batch-resolve SHA-1 hashes against Modrinth's `version_files`
/// endpoint. Returns `(matches, succeeded)`: `matches` is `sha1
/// (lowercased) -> VersionRef` for every hash the platform knows;
/// `succeeded` is `true` iff the platform returned a parsed-OK 2xx
/// response (with or without matches). Failures (transport, non-2xx,
/// decode) yield `(empty, false)`. The empty-input short-circuit yields
/// `(empty, true)` — vacuous success, no failure occurred. Best-effort:
/// any failure logs to stderr and is treated as "no matches." No API
/// key required.
async fn resolve_modrinth(base: &str, shas: &[String]) -> (HashMap<String, MrMatch>, bool) {
    let mut out = HashMap::new();
    if shas.is_empty() {
        return (out, true);
    }
    let url = format!("{base}/v2/version_files");
    let body = serde_json::to_vec(&serde_json::json!({
        "hashes": shas,
        "algorithm": "sha1",
    }))
    .expect("a fixed-shape JSON object always serializes");
    let resp = match crate::network::request::post(
        &url,
        &[("user-agent", UA), ("content-type", "application/json")],
        &body,
        "mods",
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            crate::diag!("[enrich] modrinth version_files query failed: {e}");
            return (out, false);
        }
    };
    if !(200..300).contains(&resp.status) {
        crate::diag!("[enrich] modrinth version_files HTTP {}", resp.status);
        return (out, false);
    }
    let map: HashMap<String, MrVersionLite> = match serde_json::from_slice(&resp.body) {
        Ok(m) => m,
        Err(e) => {
            crate::diag!("[enrich] modrinth version_files decode failed: {e}");
            return (out, false);
        }
    };
    for (sha, v) in map {
        out.insert(
            sha.to_ascii_lowercase(),
            MrMatch {
                project_id: v.project_id,
                version_id: v.id,
                loaders: v.loaders,
                game_versions: v.game_versions,
            },
        );
    }
    (out, true)
}

#[derive(Deserialize)]
struct CfFingerprintEnvelope {
    data: CfFingerprintData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintData {
    exact_matches: Vec<CfFingerprintMatch>,
}

#[derive(Deserialize)]
struct CfFingerprintMatch {
    /// CurseForge `exactMatches[].id` carries `modId` (same as
    /// `file.modId`), NOT the Murmur2 fingerprint we sent. The
    /// fingerprint comes back at `file.fileFingerprint`. The field is
    /// deserialised for clarity but ignored — the lookup uses
    /// `file.file_fingerprint`. (Earlier code keyed `fp_to_sha` lookups
    /// off this field and silently dropped every CF match.)
    #[allow(dead_code)]
    id: u32,
    file: CfFingerprintFile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintFile {
    /// CurseForge file id == the version identity.
    id: u32,
    /// CurseForge mod id == the project identity.
    mod_id: u32,
    /// The Murmur2 fingerprint we sent, echoed back here. Used to map
    /// this match to the jar it represents.
    file_fingerprint: u32,
}

/// Batch-resolve Murmur2 fingerprints against CurseForge's
/// `fingerprints` endpoint. `fingerprints` pairs each jar's fingerprint
/// with its SHA-1; the returned `matches` map is keyed by SHA-1
/// (lowercased) so the caller can match it to the registry.
/// `succeeded` is `true` iff CF returned a parsed-OK 2xx response (with
/// or without exactMatches). Failures (transport, non-2xx including
/// 401/403, decode) yield `(empty, false)`. The empty-input
/// short-circuit yields `(empty, true)`. Best-effort: any failure logs
/// to stderr. On 401/403 the stored key is deliberately NOT cleared —
/// enrichment runs in the background and must not disrupt the user's
/// interactive CurseForge session.
async fn resolve_curseforge(
    base: &str,
    key: &str,
    fingerprints: &[(u32, String)],
) -> (HashMap<String, VersionRef>, bool) {
    let mut out = HashMap::new();
    if fingerprints.is_empty() {
        return (out, true);
    }
    // Two jars colliding on the 32-bit Murmur2 fingerprint is
    // astronomically unlikely at modpack scale (< ~1000 jars). If it
    // ever happened, this HashMap keeps one entry and the other jar's
    // match is silently dropped — still best-effort, still no panic:
    // the dropped jar just stays unenriched.
    let fp_to_sha: HashMap<u32, String> = fingerprints
        .iter()
        .map(|(fp, sha)| (*fp, sha.clone()))
        .collect();
    let fps: Vec<u32> = fingerprints.iter().map(|(fp, _)| *fp).collect();
    let url = format!("{base}/v1/fingerprints");
    let body = serde_json::to_vec(&serde_json::json!({ "fingerprints": fps }))
        .expect("a fixed-shape JSON object always serializes");
    let resp = match crate::network::request::post(
        &url,
        &[("x-api-key", key), ("content-type", "application/json")],
        &body,
        "mods",
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            crate::diag!("[enrich] curseforge fingerprints query failed: {e}");
            return (out, false);
        }
    };
    if !(200..300).contains(&resp.status) {
        crate::diag!("[enrich] curseforge fingerprints HTTP {}", resp.status);
        return (out, false);
    }
    let parsed: CfFingerprintEnvelope = match serde_json::from_slice(&resp.body) {
        Ok(p) => p,
        Err(e) => {
            crate::diag!("[enrich] curseforge fingerprints decode failed: {e}");
            return (out, false);
        }
    };
    for m in parsed.data.exact_matches {
        if let Some(sha) = fp_to_sha.get(&m.file.file_fingerprint) {
            out.insert(
                sha.to_ascii_lowercase(),
                VersionRef {
                    source: ModSource::Curseforge,
                    project_id: m.file.mod_id.to_string(),
                    version_id: m.file.id.to_string(),
                },
            );
        }
    }
    (out, true)
}

/// Shared hash-enrichment body for a fixed set of in-scope mods. Builds
/// the SHA-1 list (Modrinth) and `(Murmur2, SHA-1)` fingerprint pairs
/// (CurseForge) from the on-disk jars, batch-queries Modrinth and — when
/// a CurseForge key is supplied — CurseForge, merges per-jar with the
/// `home`-platform tie-break, computes the `attempted` set (gated on
/// every TRIED platform returning a parsed-OK response), persists the
/// recovered identities, and returns the count newly resolved.
///
/// This is the reusable core behind both public entries:
/// `enrich_instance` (modpack-origin scope, `home = pack_origin.source`)
/// and `enrich_untracked` (all untracked mods, `home = Modrinth`). A
/// no-op (`Ok(0)`) when `in_scope` is empty. Best-effort: network
/// failures degrade to "no match"; only a registry write failure is
/// `Err`.
async fn enrich_selected(
    instance_root: &Path,
    in_scope: &[&InstalledMod],
    home: ModSource,
    instance: Option<(LoaderKind, String)>,
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
) -> Result<u32, Error> {
    if in_scope.is_empty() {
        return Ok(0);
    }

    // SHA-1s for the Modrinth query; (Murmur2, SHA-1) pairs for CF. The
    // jar on disk is `filename` (enabled) or `filename.disabled`.
    let mods_dir = installed::mods_dir(instance_root);
    let shas: Vec<String> = in_scope
        .iter()
        .map(|m| m.sha1.to_ascii_lowercase())
        .collect();
    let mut fingerprints: Vec<(u32, String)> = Vec::new();
    for m in in_scope {
        let path = if m.enabled {
            mods_dir.join(&m.filename)
        } else {
            mods_dir.join(format!("{}.disabled", m.filename))
        };
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                fingerprints.push((curseforge_fingerprint(&bytes), m.sha1.to_ascii_lowercase()));
            }
            Err(e) => {
                crate::diag!(
                    "[enrich] cannot read {} for fingerprinting: {e}",
                    path.display()
                );
            }
        }
    }

    let cf_tried = cf_key.is_some();
    let (mr, mr_ok) = resolve_modrinth(modrinth_base, &shas).await;
    let (cf, cf_ok) = match cf_key {
        Some(k) => resolve_curseforge(cf_base, k, &fingerprints).await,
        None => (HashMap::new(), true),
    };

    let mut resolved: HashMap<String, ResolvedIdentity> = HashMap::new();
    for m in in_scope {
        let key = m.sha1.to_ascii_lowercase();
        let mr_match = mr.get(&key);
        let Some(picked) = pick_identity(
            mr_match.map(MrMatch::version_ref),
            cf.get(&key).cloned(),
            home,
        ) else {
            continue;
        };
        // For a Modrinth pick, drop the version_id when the matched version's
        // loader/MC tags don't match the instance: a byte-identical jar can be
        // attached to several versions and `version_files` returns an arbitrary
        // one, so the project is trustworthy but the version is not. CurseForge
        // matches are per-file (unambiguous) and never downgraded. When the
        // instance is unknown (instance.json unreadable) we keep the version —
        // never worse than before.
        let version_id = match (picked.source, instance.as_ref(), mr_match) {
            (ModSource::Modrinth, Some((loader, mc)), Some(mm))
                if !modrinth_version_matches_instance(
                    &mm.loaders,
                    &mm.game_versions,
                    *loader,
                    mc,
                ) =>
            {
                None
            }
            _ => Some(picked.version_id),
        };
        resolved.insert(
            key,
            ResolvedIdentity {
                source: picked.source,
                project_id: picked.project_id,
                version_id,
            },
        );
    }

    // `attempted` gates on whether every platform we TRIED in this pass
    // succeeded with a parsed-OK response. A platform we did not query
    // (CF when `cf_key.is_none()`) does not count as tried. When any
    // tried platform failed, no mod gets the attempted flag — the next
    // pass retries. The `resolved` map is written back regardless so
    // partial-success identities (Modrinth hit while CF was down) still
    // land. (Spec §1-§2.)
    let all_tried_succeeded = mr_ok && (!cf_tried || cf_ok);
    let attempted: HashSet<String> = if all_tried_succeeded {
        shas.into_iter().collect()
    } else {
        HashSet::new()
    };
    installed::apply_enrichment(instance_root, &resolved, &attempted).await?;
    Ok(resolved.len() as u32)
}

/// Hash-enrich an instance's modpack override-bundled mods. Reads the
/// registry, selects the in-scope mods (see "Scope" in the plan),
/// batch-queries Modrinth and — when a CurseForge key is supplied —
/// CurseForge, merges the results with the home-platform tie-break, and
/// persists the recovered identity. Returns the number of mods newly
/// resolved. A no-op (returns `Ok(0)`) for instances with no
/// `pack_origin` or no in-scope mods. Best-effort: network failures
/// degrade to "no match"; only a registry read/write failure is `Err`.
pub async fn enrich_instance(
    instance_root: &Path,
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
) -> Result<u32, Error> {
    // `list` reconciles `mods/` into the registry, so override jars that
    // landed source=None are present before we select.
    let installed_mods = installed::list(instance_root).await?;
    let Some(pack_origin) = installed::get_pack_origin(instance_root).await? else {
        return Ok(0); // not a modpack-imported instance — never touched
    };

    let in_scope: Vec<&InstalledMod> = installed_mods
        .iter()
        .filter(|m| {
            m.source.is_none()
                && !m.enrich_attempted
                && crate::mods::updates::is_pack_origin_mod(m, Some(&pack_origin))
        })
        .collect();

    let instance = read_instance_loader_mc(instance_root).await;
    enrich_selected(
        instance_root,
        &in_scope,
        pack_origin.source,
        instance,
        modrinth_base,
        cf_base,
        cf_key,
    )
    .await
}

/// Hash-enrich EVERY untracked mod in an instance, regardless of
/// `pack_origin`. This is the launcher-import counterpart to
/// [`enrich_instance`]: instances imported from another launcher (Prism,
/// a generic `.minecraft`) have no `pack_origin` and their loose jars
/// land `source = None`, so they rely entirely on hash-enrich to recover
/// a Modrinth/CurseForge identity (the headline goal — per-mod updates).
/// `enrich_instance` would no-op for them because it gates on
/// `pack_origin`; this entry does not.
///
/// Selects in-scope = every mod with `source.is_none() &&
/// !enrich_attempted` (no pack-origin filter, no pack_origin
/// requirement) and uses `home = Modrinth` as the tie-break when a jar
/// matches on both platforms (sensible default for imports — prefer the
/// open API). Returns the number of mods newly resolved. Best-effort: a
/// missing `pack_origin` never short-circuits it; network failures
/// degrade to "no match"; only a registry read/write failure is `Err`.
pub async fn enrich_untracked(
    instance_root: &Path,
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
) -> Result<u32, Error> {
    // `list` reconciles `mods/` into the registry, so loose jars that
    // landed source=None are present before we select.
    let installed_mods = installed::list(instance_root).await?;

    let in_scope: Vec<&InstalledMod> = installed_mods
        .iter()
        .filter(|m| m.source.is_none() && !m.enrich_attempted)
        .collect();

    let instance = read_instance_loader_mc(instance_root).await;
    enrich_selected(
        instance_root,
        &in_scope,
        ModSource::Modrinth,
        instance,
        modrinth_base,
        cf_base,
        cf_key,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use sha1::{Digest, Sha1};
    use std::path::Path;
    use tempfile::TempDir;

    use crate::mods::installed::{self, PackOrigin, PackOriginFile};
    use crate::mods::modpack::schema::EnvSupport;

    /// Place a jar in the instance's `mods/` dir; return its SHA-1.
    async fn place_jar(instance_root: &Path, filename: &str, bytes: &[u8]) -> String {
        let dir = installed::mods_dir(instance_root);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join(filename), bytes).await.unwrap();
        hex::encode(Sha1::digest(bytes))
    }

    /// Write a minimal `instance.json` so `read_instance_loader_mc` can read the
    /// instance's loader + MC. `loader` is the snake_case serde value
    /// (`forge` / `fabric` / `quilt` / `neoforge` / `vanilla`).
    async fn write_instance_json(instance_root: &Path, loader: &str, mc: &str) {
        let json = format!(
            r#"{{"id":"t","name":"t","mc_version":"{mc}","loader":"{loader}","loader_version":null,"max_heap_mb":2048,"extra_jvm_args":"","created_unix_ms":0.0}}"#
        );
        tokio::fs::write(instance_root.join("instance.json"), json)
            .await
            .unwrap();
    }

    /// A `pack_origin` declaring `files` as `mods/` override entries.
    fn origin(home: ModSource, files: &[(&str, &str)]) -> PackOrigin {
        PackOrigin {
            project_id: None,
            source: home,
            project_name: "Test Pack".into(),
            version: "1.0".into(),
            files: files
                .iter()
                .map(|(sha, filename)| PackOriginFile {
                    sha1: (*sha).into(),
                    name: filename.trim_end_matches(".jar").into(),
                    filename: (*filename).into(),
                    install_path: format!("mods/{filename}"),
                    url: String::new(),
                    size: 1.0,
                    project_id: String::new(),
                    version_id: String::new(),
                    env_client: EnvSupport::Required,
                    source: home,
                })
                .collect(),
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        }
    }

    /// Build a CurseForge `/v1/fingerprints` response body matching the
    /// real API shape: top-level `exactMatches[].id` is the modId, and
    /// the fingerprint we sent is echoed at `file.fileFingerprint`.
    /// (Earlier tests put the fingerprint at the top-level `id`, which
    /// happened to match the buggy code's misread of that field.)
    fn cf_match_body(fp: u32, mod_id: u32, file_id: u32) -> serde_json::Value {
        serde_json::json!({
            "data": { "exactMatches": [
                {
                    "id": mod_id,
                    "file": {
                        "id": file_id,
                        "modId": mod_id,
                        "fileFingerprint": fp,
                    }
                }
            ]}
        })
    }

    fn vref(source: ModSource, pid: &str) -> VersionRef {
        VersionRef {
            source,
            project_id: pid.into(),
            version_id: "v".into(),
        }
    }

    #[test]
    fn pick_identity_modrinth_only() {
        let got = pick_identity(
            Some(vref(ModSource::Modrinth, "mr")),
            None,
            ModSource::Modrinth,
        );
        assert_eq!(got.unwrap().project_id, "mr");
    }

    #[test]
    fn pick_identity_curseforge_only() {
        let got = pick_identity(
            None,
            Some(vref(ModSource::Curseforge, "cf")),
            ModSource::Modrinth,
        );
        assert_eq!(got.unwrap().project_id, "cf");
    }

    #[test]
    fn pick_identity_both_home_curseforge_wins() {
        let got = pick_identity(
            Some(vref(ModSource::Modrinth, "mr")),
            Some(vref(ModSource::Curseforge, "cf")),
            ModSource::Curseforge,
        );
        assert_eq!(got.unwrap().project_id, "cf");
    }

    #[test]
    fn pick_identity_both_home_modrinth_wins() {
        let got = pick_identity(
            Some(vref(ModSource::Modrinth, "mr")),
            Some(vref(ModSource::Curseforge, "cf")),
            ModSource::Modrinth,
        );
        assert_eq!(got.unwrap().project_id, "mr");
    }

    #[test]
    fn pick_identity_no_match() {
        assert!(pick_identity(None, None, ModSource::Modrinth).is_none());
    }

    #[test]
    fn fingerprint_empty_input() {
        assert_eq!(curseforge_fingerprint(b""), 1540447798);
    }

    #[test]
    fn fingerprint_known_vector() {
        // MurmurHash2(seed=1) of "hello" — no whitespace to strip.
        assert_eq!(curseforge_fingerprint(b"hello"), 2788266382);
    }

    #[test]
    fn fingerprint_strips_whitespace_bytes() {
        // 0x09 (tab), 0x0a (LF), 0x0d (CR), 0x20 (space) are removed
        // before hashing, so these two inputs collide.
        assert_eq!(
            curseforge_fingerprint(b"a b\tc\nd"),
            curseforge_fingerprint(b"abcd"),
        );
        assert_eq!(curseforge_fingerprint(b"abcd"), 3376380438);
    }

    #[tokio::test]
    async fn resolve_modrinth_maps_hash_to_identity() {
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "abc123": {
                "id": "MR-VER",
                "project_id": "MR-PROJ",
                "loaders": ["fabric"],
                "game_versions": ["1.20.1"]
            }
        });
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) = resolve_modrinth(&s.uri(), &["abc123".to_string()]).await;
        assert!(succeeded, "200 + matches should be succeeded=true");
        let id = got.get("abc123").expect("hash should resolve");
        assert_eq!(id.project_id, "MR-PROJ");
        assert_eq!(id.version_id, "MR-VER");
        assert_eq!(id.loaders, vec!["fabric".to_string()]);
        assert_eq!(id.game_versions, vec!["1.20.1".to_string()]);
    }

    #[tokio::test]
    async fn resolve_modrinth_empty_input_makes_no_request() {
        // Empty hashes → no request, vacuous success.
        let (got, succeeded) = resolve_modrinth("http://127.0.0.1:1", &[]).await;
        assert!(got.is_empty());
        assert!(succeeded, "no failure occurred — succeeded should be true");
    }

    #[tokio::test]
    async fn resolve_modrinth_non_2xx_yields_empty_map_and_succeeded_false() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) = resolve_modrinth(&s.uri(), &["abc123".to_string()]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "non-2xx should be succeeded=false");
    }

    #[tokio::test]
    async fn resolve_modrinth_200_with_no_matches_is_succeeded_true() {
        // A successful 2xx with an empty object body means "platform
        // definitively does not know this hash" — that IS a successful
        // attempt; the mod's attempted flag should flip.
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) = resolve_modrinth(&s.uri(), &["abc123".to_string()]).await;
        assert!(got.is_empty());
        assert!(succeeded, "200 + empty object should be succeeded=true");
    }

    #[tokio::test]
    async fn resolve_modrinth_transport_failure_is_succeeded_false() {
        // Port 1 is unreachable — connection fails before any status.
        // The seam scope installs the host override in-process for this
        // test only and reverts on drop, serialised process-wide.
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_modrinth("http://127.0.0.1:1", &["abc123".to_string()]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "transport failure should be succeeded=false");
    }

    #[tokio::test]
    async fn resolve_modrinth_200_with_bad_body_is_succeeded_false() {
        // A 200 response whose body is not the expected
        // `HashMap<String, MrVersionLite>` shape (e.g. an HTML error
        // page slipped through by the CDN) is a decode failure —
        // logged and reported as succeeded=false so the mod's attempted
        // flag does not flip and the next pass retries.
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) = resolve_modrinth(&s.uri(), &["abc123".to_string()]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "decode failure should be succeeded=false");
    }

    #[tokio::test]
    async fn resolve_curseforge_maps_fingerprint_to_identity() {
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": {
                "exactMatches": [
                    {
                        "id": 12345,
                        "file": {
                            "id": 999,
                            "modId": 12345,
                            "fileFingerprint": 111
                        }
                    }
                ]
            }
        });
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_curseforge(&s.uri(), "test-key", &[(111u32, "sha-a".to_string())]).await;
        assert!(succeeded, "200 + matches should be succeeded=true");
        let id = got
            .get("sha-a")
            .expect("fingerprint should map back to its sha1");
        assert_eq!(id.source, ModSource::Curseforge);
        assert_eq!(id.project_id, "12345");
        assert_eq!(id.version_id, "999");
    }

    #[tokio::test]
    async fn resolve_curseforge_empty_input_makes_no_request() {
        let (got, succeeded) = resolve_curseforge("http://127.0.0.1:1", "k", &[]).await;
        assert!(got.is_empty());
        assert!(succeeded, "no failure occurred — succeeded should be true");
    }

    #[tokio::test]
    async fn resolve_curseforge_unauthorized_yields_empty_map_and_succeeded_false() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_curseforge(&s.uri(), "k", &[(111u32, "sha-a".to_string())]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "403 should be succeeded=false");
    }

    #[tokio::test]
    async fn resolve_curseforge_200_with_no_matches_is_succeeded_true() {
        let s = MockServer::start().await;
        let body = serde_json::json!({ "data": { "exactMatches": [] } });
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_curseforge(&s.uri(), "k", &[(111u32, "sha-a".to_string())]).await;
        assert!(got.is_empty());
        assert!(
            succeeded,
            "200 + empty exactMatches should be succeeded=true"
        );
    }

    #[tokio::test]
    async fn resolve_curseforge_401_is_succeeded_false() {
        // 401 is the same class of failure as 403 — auth rejected. The
        // key is deliberately NOT cleared (enrichment runs in the
        // background and must not disrupt the user's interactive CF
        // session); the mod's attempted flag stays unflipped.
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_curseforge(&s.uri(), "k", &[(111u32, "sha-a".to_string())]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "401 should be succeeded=false");
    }

    #[tokio::test]
    async fn resolve_curseforge_transport_failure_is_succeeded_false() {
        // Port 1 is unreachable — connection fails before any status.
        // The seam scope installs the host override in-process for this
        // test only and reverts on drop, serialised process-wide.
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_curseforge("http://127.0.0.1:1", "k", &[(111u32, "sha-a".to_string())]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "transport failure should be succeeded=false");
    }

    #[tokio::test]
    async fn resolve_curseforge_200_with_bad_body_is_succeeded_false() {
        // A 200 with a body that doesn't match the expected envelope
        // (decode failure) reports succeeded=false so the mod's
        // attempted flag does not flip — parallel to the Modrinth case.
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let (got, succeeded) =
            resolve_curseforge(&s.uri(), "k", &[(111u32, "sha-a".to_string())]).await;
        assert!(got.is_empty());
        assert!(!succeeded, "decode failure should be succeeded=false");
    }

    #[tokio::test]
    async fn enrich_instance_resolves_a_modrinth_override_mod() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "sodium.jar", b"sodium-jar-bytes").await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Modrinth, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        let mut map = serde_json::Map::new();
        map.insert(
            sha.clone(),
            serde_json::json!({ "id": "MR-VER", "project_id": "MR-PROJ" }),
        );
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Object(map)))
            .mount(&mr)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let n = enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        assert_eq!(n, 1);
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert_eq!(m.source, Some(ModSource::Modrinth));
        assert_eq!(m.project_id.as_deref(), Some("MR-PROJ"));
        assert_eq!(m.version_id.as_deref(), Some("MR-VER"));
        assert!(m.enrich_attempted);
    }

    #[tokio::test]
    async fn enrich_untracked_resolves_a_loose_mod_without_pack_origin() {
        // The launcher-import scenario: a loose jar copied from another
        // launcher lands in the registry as source=None with NO
        // pack_origin. enrich_instance would no-op (it gates on
        // pack_origin); enrich_untracked must still recover the identity.
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "sodium.jar", b"sodium-jar-bytes").await;
        // No set_pack_origin — a launcher-imported / manually-built instance.

        let mr = MockServer::start().await;
        let mut map = serde_json::Map::new();
        map.insert(
            sha.clone(),
            serde_json::json!({ "id": "MR-VER", "project_id": "MR-PROJ" }),
        );
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Object(map)))
            .mount(&mr)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // Sanity: the pack-origin-gated entry is a no-op here.
        let gated = enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        let n = enrich_untracked(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        assert_eq!(gated, 0, "enrich_instance must no-op without pack_origin");
        assert_eq!(n, 1, "enrich_untracked must recover the loose mod");
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert_eq!(m.source, Some(ModSource::Modrinth));
        assert_eq!(m.project_id.as_deref(), Some("MR-PROJ"));
        assert_eq!(m.version_id.as_deref(), Some("MR-VER"));
        assert!(m.enrich_attempted);
    }

    #[tokio::test]
    async fn enrich_instance_second_run_is_a_noop() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "sodium.jar", b"sodium-jar-bytes").await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Modrinth, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();
        let mr = MockServer::start().await;
        let mut map = serde_json::Map::new();
        map.insert(
            sha.clone(),
            serde_json::json!({ "id": "V", "project_id": "P" }),
        );
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Object(map)))
            .mount(&mr)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let first = enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        // Second run: the mod now has a source AND enrich_attempted is
        // true — either condition alone puts it out of scope.
        let second = enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 0);
    }

    #[tokio::test]
    async fn enrich_instance_skips_hand_dropped_mod() {
        let td = TempDir::new().unwrap();
        let pack_sha = place_jar(td.path(), "packmod.jar", b"pack-mod-bytes").await;
        let hand_sha = place_jar(td.path(), "handmod.jar", b"hand-dropped-bytes").await;
        // Only packmod.jar is declared in pack_origin.
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Modrinth, &[(&pack_sha, "packmod.jar")]),
        )
        .await
        .unwrap();
        let mr = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mr)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        let mods = installed::list(td.path()).await.unwrap();
        let hand = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&hand_sha))
            .unwrap();
        // A hand-dropped jar is never touched: not attempted, no identity.
        assert!(hand.source.is_none());
        assert!(!hand.enrich_attempted);
    }

    #[tokio::test]
    async fn enrich_instance_without_pack_origin_returns_zero() {
        let td = TempDir::new().unwrap();
        place_jar(td.path(), "manual.jar", b"manual-bytes").await;
        // No set_pack_origin call — a manually-created instance.
        let n = enrich_instance(td.path(), "http://127.0.0.1:1", "http://127.0.0.1:1", None)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn enrich_instance_tie_break_prefers_pack_home_platform() {
        let td = TempDir::new().unwrap();
        let bytes = b"matches-on-both-platforms";
        let sha = place_jar(td.path(), "both.jar", bytes).await;
        // Pack home platform is CurseForge.
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Curseforge, &[(&sha, "both.jar")]),
        )
        .await
        .unwrap();
        let fp = curseforge_fingerprint(bytes);

        let mr = MockServer::start().await;
        let mut map = serde_json::Map::new();
        map.insert(
            sha.clone(),
            serde_json::json!({ "id": "MR-V", "project_id": "MR-P" }),
        );
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Object(map)))
            .mount(&mr)
            .await;
        let cf = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(cf_match_body(fp, 12345, 999)))
            .mount(&cf)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        enrich_instance(td.path(), &mr.uri(), &cf.uri(), Some("test-key"))
            .await
            .unwrap();
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        // Both platforms matched; home == CurseForge, so CF wins.
        assert_eq!(m.source, Some(ModSource::Curseforge));
        assert_eq!(m.project_id.as_deref(), Some("12345"));
        assert_eq!(m.version_id.as_deref(), Some("999"));
    }

    #[tokio::test]
    async fn enrich_instance_does_not_mark_attempted_when_cf_fails() {
        // Both platforms in scope (CF key configured). Modrinth returns
        // 2xx-empty (succeeded, no match). CF returns 5xx (failed). The
        // mod must stay source=None AND enrich_attempted=false so the
        // next pass retries.
        let td = TempDir::new().unwrap();
        let bytes = b"sodium-jar-bytes";
        let sha = place_jar(td.path(), "sodium.jar", bytes).await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Curseforge, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&mr)
            .await;
        let cf = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&cf)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let n = enrich_instance(td.path(), &mr.uri(), &cf.uri(), Some("test-key"))
            .await
            .unwrap();
        assert_eq!(n, 0);
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(m.source.is_none());
        assert!(
            !m.enrich_attempted,
            "CF failed → mod must NOT be marked attempted so next pass retries"
        );
    }

    #[tokio::test]
    async fn enrich_instance_does_not_mark_attempted_when_modrinth_fails() {
        // Modrinth returns 5xx; CF returns 2xx-empty. Mirror of the
        // above. The mod stays attempted=false.
        let td = TempDir::new().unwrap();
        let bytes = b"sodium-jar-bytes";
        let sha = place_jar(td.path(), "sodium.jar", bytes).await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Curseforge, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mr)
            .await;
        let cf = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": { "exactMatches": [] } })),
            )
            .mount(&cf)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let n = enrich_instance(td.path(), &mr.uri(), &cf.uri(), Some("test-key"))
            .await
            .unwrap();
        assert_eq!(n, 0);

        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(m.source.is_none());
        assert!(!m.enrich_attempted);
    }

    #[tokio::test]
    async fn enrich_instance_marks_attempted_when_only_modrinth_tried_and_succeeded() {
        // No CF key configured. Modrinth returns 2xx-empty. Since CF
        // was never tried, the "every tried platform succeeded" gate
        // passes — the mod becomes enrich_attempted=true.
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "sodium.jar", b"sodium-jar-bytes").await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Modrinth, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&mr)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(m.source.is_none());
        assert!(
            m.enrich_attempted,
            "no-CF-key pass should still mark attempted"
        );
    }

    #[tokio::test]
    async fn enrich_instance_marks_attempted_when_both_platforms_succeed_no_matches() {
        // Both platforms in scope (CF key configured). Both return
        // 2xx-empty. The mod really is unidentifiable — mark attempted
        // so the backfill stops retrying.
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "sodium.jar", b"sodium-jar-bytes").await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Curseforge, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&mr)
            .await;
        let cf = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": { "exactMatches": [] } })),
            )
            .mount(&cf)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        enrich_instance(td.path(), &mr.uri(), &cf.uri(), Some("test-key"))
            .await
            .unwrap();
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(m.source.is_none(), "no matches → source must remain None");
        assert!(m.enrich_attempted);
    }

    #[test]
    fn version_matches_instance_exact() {
        assert!(modrinth_version_matches_instance(
            &["forge".into()],
            &["1.20.4".into()],
            LoaderKind::Forge,
            "1.20.4",
        ));
    }

    #[test]
    fn version_matches_instance_multi_loader_universal() {
        // A "universal" version listing several loaders matches as long as it
        // includes the instance loader.
        assert!(modrinth_version_matches_instance(
            &["fabric".into(), "forge".into(), "quilt".into()],
            &["1.20.4".into(), "1.20.1".into()],
            LoaderKind::Forge,
            "1.20.4",
        ));
    }

    #[test]
    fn version_mismatch_on_loader() {
        assert!(!modrinth_version_matches_instance(
            &["fabric".into()],
            &["1.20.4".into()],
            LoaderKind::Forge,
            "1.20.4",
        ));
    }

    #[test]
    fn version_mismatch_on_mc() {
        assert!(!modrinth_version_matches_instance(
            &["forge".into()],
            &["1.20.1".into()],
            LoaderKind::Forge,
            "1.20.4",
        ));
    }

    #[test]
    fn version_mismatch_on_empty_loaders() {
        assert!(!modrinth_version_matches_instance(
            &[],
            &["1.20.4".into()],
            LoaderKind::Forge,
            "1.20.4",
        ));
    }

    #[tokio::test]
    async fn enrich_instance_keeps_version_when_tags_match() {
        // instance.json says Forge/1.20.1; the matched version lists forge +
        // 1.20.1 → full identity (version_id recorded).
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "sodium.jar", b"sodium-jar-bytes").await;
        write_instance_json(td.path(), "forge", "1.20.1").await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Modrinth, &[(&sha, "sodium.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        let mut map = serde_json::Map::new();
        map.insert(
            sha.clone(),
            serde_json::json!({
                "id": "MR-VER",
                "project_id": "MR-PROJ",
                "loaders": ["forge"],
                "game_versions": ["1.20.1"]
            }),
        );
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Object(map)))
            .mount(&mr)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert_eq!(m.source, Some(ModSource::Modrinth));
        assert_eq!(m.project_id.as_deref(), Some("MR-PROJ"));
        assert_eq!(m.version_id.as_deref(), Some("MR-VER"));
        assert!(m.enrich_attempted);
    }

    #[tokio::test]
    async fn enrich_instance_records_project_only_on_loader_mismatch() {
        // instance.json says Forge/1.20.1; version_files returned a fabric
        // version (a shared "universal" jar). The project is recorded for the
        // icon, but version_id is dropped so update-check stays honest.
        let td = TempDir::new().unwrap();
        let sha = place_jar(td.path(), "universal.jar", b"universal-jar-bytes").await;
        write_instance_json(td.path(), "forge", "1.20.1").await;
        installed::set_pack_origin(
            td.path(),
            origin(ModSource::Modrinth, &[(&sha, "universal.jar")]),
        )
        .await
        .unwrap();

        let mr = MockServer::start().await;
        let mut map = serde_json::Map::new();
        map.insert(
            sha.clone(),
            serde_json::json!({
                "id": "MR-VER",
                "project_id": "MR-PROJ",
                "loaders": ["fabric"],
                "game_versions": ["1.20.1"]
            }),
        );
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Object(map)))
            .mount(&mr)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        enrich_instance(td.path(), &mr.uri(), "http://127.0.0.1:1", None)
            .await
            .unwrap();
        let mods = installed::list(td.path()).await.unwrap();
        let m = mods
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert_eq!(
            m.source,
            Some(ModSource::Modrinth),
            "project still recorded"
        );
        assert_eq!(m.project_id.as_deref(), Some("MR-PROJ"));
        assert_eq!(
            m.version_id, None,
            "loader/MC-mismatched version must NOT be recorded"
        );
        assert!(m.enrich_attempted);
    }
}
