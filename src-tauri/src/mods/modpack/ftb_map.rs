//! Pure mapper: FTB version manifest (`FtbVersion`) → `ModpackSummary`.
//!
//! No I/O, no async, no network. `map_version` is the single public entry
//! point; everything else is a private helper. Tests are co-located below.

use crate::mods::modpack::ftb_api::{FtbTarget, FtbVersion};
use crate::mods::modpack::path_safety::is_safe_relative_path;
use crate::mods::modpack::schema::{
    EnvSupport, ModpackFile, ModpackFormat, ModpackSummary, ModpackUnresolvable, UnresolvableReason,
};
use crate::mods::platform::{LoaderKind, ModSource};
use crate::network::allowlist::is_host_allowed;

// ── helpers ──────────────────────────────────────────────────────────────────

/// FTB `path` is a directory like `"./mods/"`. Join with `name`, strip the
/// leading `"./"`, trim trailing slashes, and return a normalized relative
/// path like `"mods/sodium.jar"`.
fn join_path(dir: &str, name: &str) -> String {
    let dir = dir.trim_start_matches("./").trim_end_matches('/');
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Scan `targets` for the first modloader entry; return its `LoaderKind` +
/// version. Falls back to `(Vanilla, None)` when no recognised loader is
/// listed.
fn resolve_loader(targets: &[FtbTarget]) -> (LoaderKind, Option<String>) {
    for t in targets.iter().filter(|t| t.target_type == "modloader") {
        let kind = match t.name.as_str() {
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "forge" => LoaderKind::Forge,
            "neoforge" => LoaderKind::NeoForge,
            _ => continue,
        };
        return (kind, Some(t.version.clone()));
    }
    (LoaderKind::Vanilla, None)
}

/// Build a `ModpackUnresolvable` entry.
fn unres(
    reason: UnresolvableReason,
    mod_name: impl Into<String>,
    manual_action_url: impl Into<String>,
    filename: impl Into<String>,
    size: f64,
    sha1: Option<String>,
) -> ModpackUnresolvable {
    ModpackUnresolvable {
        reason,
        mod_name: mod_name.into(),
        manual_action_url: manual_action_url.into(),
        filename: filename.into(),
        size,
        sha1,
        project_id: None,
    }
}

// ── public API ────────────────────────────────────────────────────────────────

/// Convert an FTB version manifest into the normalized `ModpackSummary`.
///
/// Rules per file:
/// 1. `serveronly` files are silently skipped (like `env_client: unsupported`
///    in Modrinth). Zero cost — the client never needs them.
/// 2. `install_path = join_path(path, name)`. Reject with `UnsafePath` if
///    the path fails the safety check.
/// 3. CurseForge-ref file (`curseforge.project != 0 && curseforge.file != 0` AND
///    empty url) → emit `ModpackFile` with `source: Curseforge`, empty url
///    placeholder. `stage_impl` runs `resolve_cf_refs` after this call to fill
///    urls via the CF bulk-files API. Unsafe paths are still rejected (step 2
///    runs first). FTB sha1 is kept if present; left empty if absent (CF API
///    will backfill it during resolution).
/// 4. Empty / whitespace `sha1` → `UnresolvableReason::HostNotAllowed` (no
///    TOFU — we never trust a file whose integrity we cannot verify). Not applied
///    to CF-ref files (covered by step 3 above).
/// 5. Host not on the allowlist → `UnresolvableReason::HostNotAllowed`.
/// 6. All others → `ModpackFile` (env_client always `Required`; FTB does not
///    expose per-file client/server env tags).
///
/// For FTB-CDN files: `project_id` is the FTB file `id`; `version_id` is the
/// sha1. For CF-ref files: `project_id` / `version_id` are the CF project/file
/// ids so the install pipeline can treat them as CF mods.
pub fn map_version(pack_name: &str, version_name: &str, v: &FtbVersion) -> ModpackSummary {
    let game_version = v
        .targets
        .iter()
        .find(|t| t.target_type == "game" && t.name == "minecraft")
        .map(|t| t.version.clone())
        .unwrap_or_default();

    let (loader, loader_version) = resolve_loader(&v.targets);

    let mut files: Vec<ModpackFile> = Vec::new();
    let mut unresolvable: Vec<ModpackUnresolvable> = Vec::new();

    for f in &v.files {
        // 1. Server-only files are not needed by the client.
        if f.serveronly {
            continue;
        }

        let install_path = join_path(&f.path, &f.name);

        // 2. Path safety.
        if !is_safe_relative_path(&install_path) {
            unresolvable.push(unres(
                UnresolvableReason::UnsafePath,
                &f.name,
                String::new(),
                &f.name,
                f.size,
                None,
            ));
            continue;
        }

        // 3. CurseForge-ref file: empty url but a valid CF project+file id.
        //    Emit as a Curseforge-sourced ModpackFile with an empty url placeholder;
        //    stage_impl will bulk-resolve these before writing the sidecar.
        //    The sha1 check (step 4) is deliberately skipped here — FTB sometimes
        //    omits sha1 for CF-ref entries; the CF API will backfill it.
        if let Some(cf) = &f.curseforge {
            if cf.project != 0 && cf.file != 0 && f.url.trim().is_empty() {
                let sha1 = if f.sha1.trim().is_empty() {
                    String::new()
                } else {
                    f.sha1.to_ascii_lowercase()
                };
                files.push(ModpackFile {
                    project_id: cf.project.to_string(),
                    version_id: cf.file.to_string(),
                    name: f.name.clone(),
                    filename: f.name.clone(),
                    install_path,
                    sha1,
                    url: String::new(), // placeholder — filled by resolve_cf_refs in stage_impl
                    size: f.size,
                    env_client: EnvSupport::Required,
                    source: ModSource::Curseforge,
                });
                continue;
            }
        }

        // 4. Missing sha1 → unresolvable (never TOFU).
        if f.sha1.trim().is_empty() {
            // DEVIATION: reuse HostNotAllowed because no MissingChecksum variant exists
            // (YAGNI — dist.modpacks.ch always supplies sha1; this is a defensive
            // near-never guard). Add UnresolvableReason::MissingChecksum if this ever
            // becomes user-visible or a second FTB CDN omits checksums.
            unresolvable.push(unres(
                UnresolvableReason::HostNotAllowed,
                &f.name,
                f.url.clone(),
                &f.name,
                f.size,
                None,
            ));
            continue;
        }

        // 5. Host allowlist check.
        let host = url::Url::parse(&f.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
        let allowed = host.as_deref().is_some_and(is_host_allowed);
        if !allowed {
            unresolvable.push(unres(
                UnresolvableReason::HostNotAllowed,
                &f.name,
                f.url.clone(),
                &f.name,
                f.size,
                Some(f.sha1.to_ascii_lowercase()),
            ));
            continue;
        }

        // 6. Accepted FTB-CDN file.
        let sha1 = f.sha1.to_ascii_lowercase();
        files.push(ModpackFile {
            project_id: f.id.to_string(),
            version_id: sha1.clone(),
            name: f.name.clone(),
            filename: f.name.clone(),
            install_path,
            sha1,
            url: f.url.clone(),
            size: f.size,
            env_client: EnvSupport::Required,
            source: ModSource::Ftb,
        });
    }

    ModpackSummary {
        format: ModpackFormat::Ftb,
        name: pack_name.to_string(),
        version: version_name.to_string(),
        game_version,
        loader,
        loader_version,
        files,
        unresolvable,
        has_overrides: false,
        has_client_overrides: false,
        has_saves_in_overrides: false,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::modpack::ftb_api::{FtbFile, FtbTarget, FtbVersion};

    fn mc_target(version: &str) -> FtbTarget {
        FtbTarget {
            name: "minecraft".into(),
            version: version.into(),
            target_type: "game".into(),
        }
    }

    fn loader_target(name: &str, version: &str) -> FtbTarget {
        FtbTarget {
            name: name.into(),
            version: version.into(),
            target_type: "modloader".into(),
        }
    }

    fn java_target(version: &str) -> FtbTarget {
        FtbTarget {
            name: "java".into(),
            version: version.into(),
            target_type: "runtime".into(),
        }
    }

    fn mod_file(
        id: u64,
        name: &str,
        path: &str,
        url: &str,
        sha1: &str,
        serveronly: bool,
    ) -> FtbFile {
        FtbFile {
            id,
            name: name.into(),
            path: path.into(),
            url: url.into(),
            sha1: sha1.into(),
            size: 512.0,
            file_type: "mod".into(),
            clientonly: false,
            serveronly,
            optional: false,
            curseforge: None,
        }
    }

    fn cf_ref_file(
        id: u64,
        name: &str,
        path: &str,
        sha1: &str,
        cf_project: u64,
        cf_file: u64,
    ) -> FtbFile {
        use crate::mods::modpack::ftb_api::FtbCfRef;
        FtbFile {
            id,
            name: name.into(),
            path: path.into(),
            url: String::new(), // empty — CF-distributed
            sha1: sha1.into(),
            size: 1024.0,
            file_type: "mod".into(),
            clientonly: false,
            serveronly: false,
            optional: false,
            curseforge: Some(FtbCfRef {
                project: cf_project,
                file: cf_file,
            }),
        }
    }

    // Helper version with dist.modpacks.ch allowed via env override.
    fn with_dist_host<F: FnOnce() -> R, R>(f: F) -> R {
        let _g = crate::test_env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "dist.modpacks.ch");
        let result = f();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        result
    }

    // ── Test 1 ────────────────────────────────────────────────────────────────

    #[test]
    fn maps_targets_to_mc_and_loader() {
        let v = FtbVersion {
            files: vec![],
            targets: vec![
                loader_target("forge", "36.2.39"),
                mc_target("1.16.5"),
                java_target("17"),
            ],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(s.game_version, "1.16.5");
        assert_eq!(s.loader, LoaderKind::Forge);
        assert_eq!(s.loader_version.as_deref(), Some("36.2.39"));
        assert_eq!(s.format, ModpackFormat::Ftb);
    }

    // ── Test 2 ────────────────────────────────────────────────────────────────

    #[test]
    fn mod_file_with_dist_host_resolves() {
        let result = with_dist_host(|| {
            let f = mod_file(
                1001,
                "sodium.jar",
                "./mods/",
                "https://dist.modpacks.ch/x/sodium.jar",
                "abc123",
                false,
            );
            let v = FtbVersion {
                files: vec![f],
                targets: vec![mc_target("1.20.1"), loader_target("fabric", "0.15.7")],
            };
            map_version("Test Pack", "1.0", &v)
        });

        assert_eq!(
            result.files.len(),
            1,
            "expected 1 file, got {:?}",
            result.files
        );
        assert!(result.unresolvable.is_empty());

        let f = &result.files[0];
        assert_eq!(f.install_path, "mods/sodium.jar");
        assert_eq!(f.sha1, "abc123");
        assert_eq!(f.source, ModSource::Ftb);
        assert_eq!(f.project_id, "1001");
        assert_eq!(f.version_id, "abc123");
    }

    // ── Test 3 ────────────────────────────────────────────────────────────────

    #[test]
    fn file_without_sha1_is_unresolvable_not_tofu() {
        let f = mod_file(
            2001,
            "nosha.jar",
            "./mods/",
            "https://dist.modpacks.ch/x/nosha.jar",
            "", // empty sha1
            false,
        );
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1")],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(s.files.len(), 0, "no-sha1 file must not be added to files");
        assert_eq!(s.unresolvable.len(), 1);
        assert!(s.unresolvable[0].sha1.is_none());
        // Documents the conscious reuse of HostNotAllowed for the missing-sha1 case
        // and guards future refactors that add a dedicated MissingChecksum variant.
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::HostNotAllowed
        ));
    }

    // ── Test 4 ────────────────────────────────────────────────────────────────

    #[test]
    fn file_with_disallowed_host_is_unresolvable() {
        let f = mod_file(
            3001,
            "evil.jar",
            "./mods/",
            "https://evil.example/evil.jar",
            "deadbeef",
            false,
        );
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1")],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(s.files.len(), 0);
        assert_eq!(s.unresolvable.len(), 1);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::HostNotAllowed
        ));
    }

    // ── Test 5 ────────────────────────────────────────────────────────────────

    #[test]
    fn serveronly_file_is_skipped() {
        let f = mod_file(
            4001,
            "server.jar",
            "./mods/",
            "https://dist.modpacks.ch/x/server.jar",
            "aabbccdd",
            true, // serveronly
        );
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1")],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(
            s.files.len(),
            0,
            "server-only file must be silently skipped"
        );
        assert_eq!(
            s.unresolvable.len(),
            0,
            "server-only file must not appear in unresolvable"
        );
    }

    // ── Test 6 ────────────────────────────────────────────────────────────────

    #[test]
    fn config_file_installs_at_joined_path() {
        let result = with_dist_host(|| {
            let f = mod_file(
                5001,
                "CBMicroblock.cfg",
                "./config/",
                "https://dist.modpacks.ch/x/CBMicroblock.cfg",
                "cafecafe",
                false,
            );
            let v = FtbVersion {
                files: vec![f],
                targets: vec![mc_target("1.12.2")],
            };
            map_version("Test Pack", "1.0", &v)
        });

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].install_path, "config/CBMicroblock.cfg");
    }

    // ── Test 7 ────────────────────────────────────────────────────────────────

    #[test]
    fn unsafe_path_is_unresolvable() {
        let f = mod_file(
            6001,
            "x.jar",
            "../",
            "https://dist.modpacks.ch/x/x.jar",
            "aa",
            false,
        );
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1")],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(s.files.len(), 0);
        assert_eq!(s.unresolvable.len(), 1);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::UnsafePath
        ));
    }

    // ── Test 8 ────────────────────────────────────────────────────────────────

    #[test]
    fn vanilla_pack_has_no_loader() {
        // Mirror of modrinth.rs's `vanilla_pack_has_no_loader_dep`.
        // An FtbVersion with only a minecraft game target and no files should
        // resolve to Vanilla with no loader version.
        let v = FtbVersion {
            files: vec![],
            targets: vec![mc_target("1.20.4")],
        };
        let s = map_version("Vanilla Test", "1.0", &v);
        assert_eq!(s.loader, LoaderKind::Vanilla);
        assert_eq!(s.loader_version, None);
        assert_eq!(s.game_version, "1.20.4");
    }

    // ── Test 9 ────────────────────────────────────────────────────────────────

    /// A file with an empty url but a valid CurseForge project+file ref must
    /// land in `files` with `source = Curseforge`, the CF ids in project_id /
    /// version_id, and NOT in `unresolvable`.
    #[test]
    fn cf_ref_file_becomes_curseforge_placeholder() {
        let f = cf_ref_file(
            9001, // FTB file id (not used for CF-ref files)
            "ae2.jar",
            "./mods/",
            "aabbccddeeff", // sha1 provided by FTB
            238222,         // CF project id
            4499899,        // CF file id
        );
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1"), loader_target("forge", "47.2.0")],
        };
        let s = map_version("Test Pack", "1.0", &v);

        assert_eq!(
            s.files.len(),
            1,
            "CF-ref file must be added to files, not skipped"
        );
        assert!(
            s.unresolvable.is_empty(),
            "CF-ref file must NOT appear in unresolvable"
        );
        let f = &s.files[0];
        assert_eq!(
            f.source,
            ModSource::Curseforge,
            "source must be Curseforge for CF-ref files"
        );
        assert_eq!(f.project_id, "238222", "project_id must be CF project id");
        assert_eq!(f.version_id, "4499899", "version_id must be CF file id");
        assert!(
            f.url.is_empty(),
            "url must be empty placeholder (resolved later by stage_impl)"
        );
        assert_eq!(
            f.sha1, "aabbccddeeff",
            "sha1 from FTB manifest must be preserved"
        );
        assert_eq!(f.install_path, "mods/ae2.jar");
    }

    /// A CF-ref file with an empty FTB sha1 should still land in files (not
    /// unresolvable) — the CF API will backfill the sha1 during resolution.
    #[test]
    fn cf_ref_file_empty_sha1_still_accepted() {
        let f = cf_ref_file(
            9002,
            "peripheral.jar",
            "./mods/",
            "",      // empty sha1
            312197,  // CF project id
            5678901, // CF file id
        );
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1")],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(s.files.len(), 1);
        assert!(s.unresolvable.is_empty());
        assert_eq!(s.files[0].source, ModSource::Curseforge);
        assert!(
            s.files[0].sha1.is_empty(),
            "empty sha1 must be passed through"
        );
    }

    /// A file with an empty url AND no CF ref should still go to unresolvable.
    #[test]
    fn empty_url_without_cf_ref_is_unresolvable() {
        // Build a file with empty url and no curseforge ref, but with a sha1.
        // This hits the host-not-allowed path (empty url fails url::Url::parse).
        use crate::mods::modpack::ftb_api::FtbFile;
        let f = FtbFile {
            id: 9003,
            name: "unknown.jar".into(),
            path: "./mods/".into(),
            url: String::new(), // empty url, no CF ref
            sha1: "deadbeef".into(),
            size: 100.0,
            file_type: "mod".into(),
            clientonly: false,
            serveronly: false,
            optional: false,
            curseforge: None,
        };
        let v = FtbVersion {
            files: vec![f],
            targets: vec![mc_target("1.20.1")],
        };
        let s = map_version("Test Pack", "1.0", &v);
        assert_eq!(s.files.len(), 0, "no CF ref → not a valid file");
        assert_eq!(
            s.unresolvable.len(),
            1,
            "no CF ref + empty url must go to unresolvable"
        );
    }

    // ── Test 10 ───────────────────────────────────────────────────────────────

    #[test]
    fn join_path_edge_cases() {
        // Empty dir: name returned as-is, no "./" prefix.
        assert_eq!(join_path("", "x.jar"), "x.jar");
        // Plain dir: joined with "/", no "./" prefix.
        assert_eq!(join_path("mods", "x.jar"), "mods/x.jar");
        // "./mods/sub/" with trailing slash: leading "./" stripped, trailing "/"
        // trimmed, yielding a clean "mods/sub/x.jar".
        assert_eq!(join_path("./mods/sub/", "x.jar"), "mods/sub/x.jar");
    }
}
