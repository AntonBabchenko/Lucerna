//! Pure mapper: ATLauncher `Configs.json` (`AtlConfigs`) → `ModpackSummary`.
//! No I/O. `map_configs` is the single entry point. CF-integration mods are
//! emitted as Curseforge placeholders (resolved later, like FTB); server/direct
//! mods carry an md5 for the installer to verify.

use crate::mods::modpack::atl_api::{AtlConfigs, AtlMod};
use crate::mods::modpack::path_safety::is_safe_relative_path;
use crate::mods::modpack::schema::{
    EnvSupport, ModpackFile, ModpackFormat, ModpackSummary, ModpackUnresolvable, UnresolvableReason,
};
use crate::mods::platform::{LoaderKind, ModSource};
use crate::network::allowlist::is_host_allowed;

/// Loader type strings that appear in legacy mod-entry-based declarations.
const LOADER_TYPES: &[&str] = &["forge", "fabric", "quilt", "neoforge"];

fn loader_kind_from_str(s: &str) -> Option<LoaderKind> {
    match s {
        "fabric" => Some(LoaderKind::Fabric),
        "quilt" => Some(LoaderKind::Quilt),
        "forge" => Some(LoaderKind::Forge),
        "neoforge" => Some(LoaderKind::NeoForge),
        _ => None,
    }
}

/// Detect the mod loader from `Configs.json`.
///
/// Modern packs declare a top-level `loader` object. Legacy packs (MC 1.7.10
/// era) have no such object — the loader is embedded as an entry in the `mods`
/// array with `type: "forge"` (or similar). We try the modern path first and
/// fall back to scanning the mods array.
fn loader_from(configs: &AtlConfigs) -> (LoaderKind, Option<String>) {
    // Modern path: explicit top-level loader object.
    if let Some(l) = &configs.loader {
        if let Some(kind) = loader_kind_from_str(l.loader_type.as_str()) {
            let ver = l
                .metadata
                .as_ref()
                .map(|m| m.version.clone())
                .filter(|v| !v.is_empty());
            return (kind, ver);
        }
    }

    // Legacy path: find the first mod entry whose type is a loader name.
    for m in &configs.mods {
        if let Some(kind) = loader_kind_from_str(m.mod_type.as_str()) {
            let ver = if m.version.is_empty() {
                None
            } else {
                Some(m.version.clone())
            };
            return (kind, ver);
        }
    }

    (LoaderKind::Vanilla, None)
}

/// Compute the absolute URL for a mod.
///
/// ATLauncher legacy packs use relative paths for `server`-download mods
/// (e.g. `"packs/BoBuddyBunkers/files/1.7.10/mods/foo.jar"`). The CDN base
/// (`https://download.nodecdn.net/containers/atl`) must be prepended to make
/// the URL usable.
fn resolve_url(cdn_base: &str, url: &str) -> String {
    if url.is_empty() || url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!(
            "{}/{}",
            cdn_base.trim_end_matches('/'),
            url.trim_start_matches('/')
        )
    }
}

/// CurseForge CDN URLs encode the file id in the path: `/files/3820/040/<name>`
/// → 3820 * 1000 + 040 = 3820040. Returns the file id for `edge.forgecdn.net`
/// and `mediafilez.forgecdn.net` hosts, else `None`.
fn forgecdn_file_id(url: &str) -> Option<u64> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if host != "edge.forgecdn.net" && host != "mediafilez.forgecdn.net" {
        return None;
    }
    let mut segs = parsed.path_segments()?;
    if segs.next()? != "files" {
        return None;
    }
    let a: u64 = segs.next()?.parse().ok()?;
    let b: u64 = segs.next()?.parse().ok()?;
    Some(a * 1000 + b)
}

/// v1 install path: ATLauncher uses `type: "mods"` for jars; route every
/// accepted mod to `mods/<file>`. (Rarer asset-extract types are out of scope
/// for v1 and never reach here as accepted files.)
fn install_path_for(m: &AtlMod) -> String {
    format!("mods/{}", m.file)
}

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

/// Map a `Configs.json` manifest into a `ModpackSummary`.
///
/// `cdn_base` is the ATLauncher NodeCDN base URL used to resolve relative
/// `server`-download URLs found in legacy pack manifests. Pass
/// `atl_api::nodecdn_base()` in production; inject a test server URL in tests.
pub fn map_configs(
    pack_name: &str,
    version_name: &str,
    cdn_base: &str,
    c: &AtlConfigs,
) -> ModpackSummary {
    let (loader, loader_version) = loader_from(c);
    let mut files: Vec<ModpackFile> = Vec::new();
    let mut unresolvable: Vec<ModpackUnresolvable> = Vec::new();

    for m in &c.mods {
        if !m.client {
            continue;
        }
        if m.optional {
            continue;
        }

        // Skip loader entries — the loader is handled by the normal loader
        // installation pipeline, not as a mod jar.
        if LOADER_TYPES.contains(&m.mod_type.as_str()) {
            continue;
        }

        // v1: config/asset-overlay entries (type "extract" etc.) are surfaced
        // as manual — auto-extracting pack overlays is a follow-up.
        if m.mod_type != "mods" {
            let resolved_url = resolve_url(cdn_base, &m.url);
            unresolvable.push(unres(
                UnresolvableReason::DistributionDisabled,
                &m.name,
                resolved_url,
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        let install_path = install_path_for(m);
        if !is_safe_relative_path(&install_path) {
            unresolvable.push(unres(
                UnresolvableReason::UnsafePath,
                &m.name,
                "",
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        if m.cf_project_id != 0 && m.cf_file_id != 0 {
            files.push(ModpackFile {
                project_id: m.cf_project_id.to_string(),
                version_id: m.cf_file_id.to_string(),
                name: m.name.clone(),
                filename: m.file.clone(),
                install_path,
                sha1: String::new(),
                url: String::new(),
                size: m.filesize,
                env_client: EnvSupport::Required,
                source: ModSource::Curseforge,
                md5: None,
            });
            continue;
        }

        if m.download == "browser" {
            let resolved_url = resolve_url(cdn_base, &m.url);
            unresolvable.push(unres(
                UnresolvableReason::DistributionDisabled,
                &m.name,
                resolved_url,
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        // Resolve relative URLs (legacy server-download mods use relative paths
        // against the NodeCDN base).
        let abs_url = resolve_url(cdn_base, &m.url);

        if m.md5.trim().is_empty() {
            // `direct` mods pointing at a CurseForge CDN carry the file id
            // embedded in the URL path but no md5 in the manifest. Emit them
            // as an ATLauncher placeholder (sha1="", md5=None, version_id =
            // CF file id) so `resolve_forgecdn_sha1` can fill the sha1 from
            // the CF API at stage time. The forgecdn URL is kept as the manual
            // fallback in case CF resolution fails.
            if let Some(file_id) = forgecdn_file_id(&abs_url) {
                files.push(ModpackFile {
                    project_id: m.file.clone(),
                    version_id: file_id.to_string(),
                    name: m.name.clone(),
                    filename: m.file.clone(),
                    install_path,
                    sha1: String::new(), // filled at stage from CF; empty → unresolvable if CF fails
                    url: abs_url.clone(), // forgecdn URL kept as manual fallback
                    size: m.filesize,
                    env_client: EnvSupport::Required,
                    source: ModSource::Atlauncher,
                    md5: None, // NOT an md5 file — resolved via CF sha1
                });
                continue;
            }
            // Non-forgecdn host with no md5 — no safe way to verify; surface
            // as manual (no-TOFU).
            unresolvable.push(unres(
                UnresolvableReason::MissingChecksum,
                &m.name,
                abs_url,
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        let host = url::Url::parse(&abs_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
        if !host.as_deref().is_some_and(is_host_allowed) {
            unresolvable.push(unres(
                UnresolvableReason::HostNotAllowed,
                &m.name,
                abs_url,
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        files.push(ModpackFile {
            project_id: m.file.clone(),
            version_id: m.md5.to_ascii_lowercase(),
            name: m.name.clone(),
            filename: m.file.clone(),
            install_path,
            // sha1 holds the md5 as a transient selection token; the installer
            // replaces it with the real computed sha1 (never persisted). See spec.
            sha1: m.md5.to_ascii_lowercase(),
            url: abs_url,
            size: m.filesize,
            env_client: EnvSupport::Required,
            source: ModSource::Atlauncher,
            md5: Some(m.md5.to_ascii_lowercase()),
        });
    }

    ModpackSummary {
        format: ModpackFormat::Atlauncher,
        name: pack_name.to_string(),
        version: version_name.to_string(),
        game_version: c.minecraft.clone(),
        loader,
        loader_version,
        files,
        unresolvable,
        has_overrides: false,
        has_client_overrides: false,
        has_saves_in_overrides: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::modpack::atl_api::{self, AtlConfigs, AtlLoader, AtlLoaderMeta, AtlMod};

    const CDN: &str = "https://download.nodecdn.net/containers/atl";

    fn base_mod(name: &str, download: &str) -> AtlMod {
        AtlMod {
            name: name.into(),
            file: format!("{name}.jar"),
            url: format!("https://download.nodecdn.net/x/{name}.jar"),
            md5: "abc123".into(),
            filesize: 100.0,
            mod_type: "mods".into(),
            download: download.into(),
            client: true,
            optional: false,
            cf_project_id: 0,
            cf_file_id: 0,
            version: String::new(),
        }
    }

    fn forge_configs(mods: Vec<AtlMod>) -> AtlConfigs {
        AtlConfigs {
            version: "1.0".into(),
            minecraft: "1.12.2".into(),
            loader: Some(AtlLoader {
                loader_type: "forge".into(),
                metadata: Some(AtlLoaderMeta {
                    version: "14.23.5.2847".into(),
                }),
            }),
            mods,
        }
    }

    /// A legacy (MC 1.7.10-era) configs with NO top-level loader object.
    /// The loader is embedded as a mod array entry.
    fn legacy_configs(mods: Vec<AtlMod>) -> AtlConfigs {
        AtlConfigs {
            version: "0.6.6".into(),
            minecraft: "1.7.10".into(),
            loader: None,
            mods,
        }
    }

    fn with_nodecdn<F: FnOnce() -> R, R>(f: F) -> R {
        // Serialize tests that touch the process-global allowlist state.
        // `download.nodecdn.net` is on the production allowlist, so no env
        // override is needed — the guard alone is sufficient.
        let _g = crate::test_env_lock();
        f()
    }

    // ---- existing tests (updated for cdn_base param and version field) --------

    #[test]
    fn maps_loader_and_mc() {
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![]));
        assert_eq!(s.format, ModpackFormat::Atlauncher);
        assert_eq!(s.game_version, "1.12.2");
        assert_eq!(s.loader, LoaderKind::Forge);
        assert_eq!(s.loader_version.as_deref(), Some("14.23.5.2847"));
    }

    #[test]
    fn server_mod_uses_md5_as_selection_token() {
        let s = with_nodecdn(|| {
            map_configs(
                "Pack",
                "1.0",
                CDN,
                &forge_configs(vec![base_mod("jei", "server")]),
            )
        });
        assert_eq!(s.files.len(), 1);
        let f = &s.files[0];
        assert_eq!(f.source, ModSource::Atlauncher);
        assert_eq!(f.md5.as_deref(), Some("abc123"));
        assert_eq!(
            f.sha1, "abc123",
            "sha1 holds the md5 as the transient selection token"
        );
        assert_eq!(f.install_path, "mods/jei.jar");
    }

    #[test]
    fn cf_mod_becomes_curseforge_placeholder() {
        let mut m = base_mod("ae2", "direct");
        m.cf_project_id = 238222;
        m.cf_file_id = 4499899;
        m.md5 = String::new();
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![m]));
        assert_eq!(s.files.len(), 1);
        assert!(s.unresolvable.is_empty());
        let f = &s.files[0];
        assert_eq!(f.source, ModSource::Curseforge);
        assert_eq!(f.project_id, "238222");
        assert_eq!(f.version_id, "4499899");
        assert!(f.url.is_empty());
    }

    #[test]
    fn browser_mod_is_unresolvable() {
        let s = map_configs(
            "Pack",
            "1.0",
            CDN,
            &forge_configs(vec![base_mod("x", "browser")]),
        );
        assert_eq!(s.files.len(), 0);
        assert_eq!(s.unresolvable.len(), 1);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::DistributionDisabled
        ));
    }

    #[test]
    fn unsafe_file_path_is_unresolvable() {
        let mut m = base_mod("x", "server");
        m.file = "../escape.jar".into();
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![m]));
        assert_eq!(s.files.len(), 0);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::UnsafePath
        ));
    }

    #[test]
    fn server_mod_without_md5_is_missing_checksum() {
        let mut m = base_mod("x", "server");
        m.md5 = String::new();
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![m]));
        assert_eq!(s.files.len(), 0);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::MissingChecksum
        ));
    }

    #[test]
    fn direct_mod_off_allowlist_is_host_not_allowed() {
        let mut m = base_mod("x", "direct");
        m.url = "https://evil.example/x.jar".into();
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![m]));
        assert_eq!(s.files.len(), 0);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::HostNotAllowed
        ));
    }

    #[test]
    fn optional_and_server_only_mods_skipped() {
        let mut opt = base_mod("opt", "server");
        opt.optional = true;
        let mut srv = base_mod("srv", "server");
        srv.client = false;
        let s = with_nodecdn(|| map_configs("Pack", "1.0", CDN, &forge_configs(vec![opt, srv])));
        assert_eq!(s.files.len(), 0);
        assert_eq!(s.unresolvable.len(), 0);
    }

    // ---- new TDD tests for the two bugs ----------------------------------------

    /// Bug A fix: a `server`-download mod with a relative URL must have the CDN
    /// base prepended so the host-allowlist check succeeds and the emitted URL
    /// is absolute. Before the fix, `url::Url::parse` failed on relative paths
    /// → host=None → `HostNotAllowed` for every server mod in old packs.
    #[test]
    fn server_mod_relative_url_is_resolved_against_cdn_base() {
        let mut m = base_mod("advancedgenetics", "server");
        // Relative path as found in Bo-Buddy-Bunkers 0.6.6 (MC 1.7.10).
        m.url = "packs/BoBuddyBunkers/files/1.7.10/mods/advancedgenetics-1.7.10-1.5.9.jar".into();
        m.md5 = "deadbeef01234567".into();
        m.file = "advancedgenetics-1.7.10-1.5.9.jar".into();

        let s = with_nodecdn(|| {
            map_configs(
                "BoBuddyBunkers",
                "0.6.6",
                atl_api::nodecdn_base(),
                &legacy_configs(vec![m]),
            )
        });

        assert_eq!(s.files.len(), 1, "mod should be accepted, not unresolvable");
        assert!(s.unresolvable.is_empty());
        let f = &s.files[0];
        assert_eq!(
            f.url,
            "https://download.nodecdn.net/containers/atl/packs/BoBuddyBunkers/files/1.7.10/mods/advancedgenetics-1.7.10-1.5.9.jar",
            "URL must be absolute"
        );
        assert_eq!(f.source, ModSource::Atlauncher);
        assert_eq!(f.md5.as_deref(), Some("deadbeef01234567"));
        assert_eq!(f.sha1, "deadbeef01234567");
    }

    /// Bug B fix: when there is NO top-level `loader` object but the mods array
    /// contains a `type: "forge"` entry, loader detection must return Forge with
    /// the version from the entry's `version` field. The forge entry must NOT
    /// appear in `files` (it is the loader, not an installable mod jar).
    #[test]
    fn legacy_loader_from_mod_entry() {
        let forge_entry = AtlMod {
            name: "Minecraft Forge".into(),
            file: "forge-1.7.10-10.13.4.1558-1.7.10-universal.jar".into(),
            url: "packs/BoBuddyBunkers/files/1.7.10/forge-1.7.10-10.13.4.1558-1.7.10-universal.jar"
                .into(),
            md5: "cafebabe".into(),
            filesize: 4_500_000.0,
            mod_type: "forge".into(),
            download: "server".into(),
            client: true,
            optional: false,
            cf_project_id: 0,
            cf_file_id: 0,
            version: "10.13.4.1558".into(),
        };
        // One real mod alongside the loader entry.
        let real_mod = AtlMod {
            name: "JEI".into(),
            file: "jei-1.7.10-2.3.6.jar".into(),
            url: "packs/BoBuddyBunkers/files/1.7.10/mods/jei-1.7.10-2.3.6.jar".into(),
            md5: "11223344".into(),
            filesize: 300_000.0,
            mod_type: "mods".into(),
            download: "server".into(),
            client: true,
            optional: false,
            cf_project_id: 0,
            cf_file_id: 0,
            version: String::new(),
        };

        let s = with_nodecdn(|| {
            map_configs(
                "BoBuddyBunkers",
                "0.6.6",
                atl_api::nodecdn_base(),
                &legacy_configs(vec![forge_entry, real_mod]),
            )
        });

        assert_eq!(
            s.loader,
            LoaderKind::Forge,
            "loader detected from mod entry"
        );
        assert_eq!(
            s.loader_version.as_deref(),
            Some("10.13.4.1558"),
            "version taken from mod entry's version field"
        );
        // The forge entry itself must NOT be in files.
        assert_eq!(s.files.len(), 1, "only the real mod jar should be in files");
        assert_eq!(s.files[0].name, "JEI");
        assert!(
            s.unresolvable.is_empty(),
            "forge loader entry must not appear in unresolvable either"
        );
    }

    /// Bug B corollary: the existing modern-loader-object path must still work
    /// (regression guard).
    #[test]
    fn modern_loader_object_still_detected() {
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![]));
        assert_eq!(s.loader, LoaderKind::Forge);
        assert_eq!(s.loader_version.as_deref(), Some("14.23.5.2847"));
    }

    /// Non-"mods" content types (e.g. `type: "extract"` config/asset overlays)
    /// must NOT be installed as mod jars. They must appear in `unresolvable` so
    /// the user can see them, with a resolved (absolute) `manual_action_url`.
    #[test]
    fn extract_type_is_surfaced_not_installed() {
        let extract_entry = AtlMod {
            name: "1710 Files".into(),
            file: "1710_files.zip".into(),
            url: "packs/BoBuddyBunkers/files/1.7.10/1710_files.zip".into(),
            md5: "aabbccdd".into(),
            filesize: 1_200_000.0,
            mod_type: "extract".into(),
            download: "server".into(),
            client: true,
            optional: false,
            cf_project_id: 0,
            cf_file_id: 0,
            version: String::new(),
        };

        let s = map_configs(
            "BoBuddyBunkers",
            "0.6.6",
            atl_api::nodecdn_base(),
            &legacy_configs(vec![extract_entry]),
        );

        assert_eq!(
            s.files.len(),
            0,
            "extract entry must not be installed as a mod jar"
        );
        assert_eq!(
            s.unresolvable.len(),
            1,
            "extract entry must appear in unresolvable"
        );
        assert_eq!(
            s.unresolvable[0].manual_action_url,
            "https://download.nodecdn.net/containers/atl/packs/BoBuddyBunkers/files/1.7.10/1710_files.zip",
            "manual_action_url must be absolute"
        );
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::DistributionDisabled
        ));
    }

    // ---- forgecdn file-id parser + forgecdn-direct branch tests ---------------

    /// `forgecdn_file_id` parses the encoded file id from both CDN hosts.
    #[test]
    fn forgecdn_file_id_parses_cf_id() {
        // edge.forgecdn.net: /files/3820/040/<name> → 3820 * 1000 + 40 = 3820040
        assert_eq!(
            forgecdn_file_id("https://edge.forgecdn.net/files/3820/040/journeymap.jar"),
            Some(3820040),
        );
        // mediafilez.forgecdn.net: same scheme
        assert_eq!(
            forgecdn_file_id("https://mediafilez.forgecdn.net/files/1234/567/y.jar"),
            Some(1234567),
        );
        // non-forgecdn host → None
        assert_eq!(
            forgecdn_file_id("https://download.nodecdn.net/files/3820/040/x.jar"),
            None,
        );
        // forgecdn url with non-numeric segment → None
        assert_eq!(
            forgecdn_file_id("https://edge.forgecdn.net/files/abc/040/x.jar"),
            None,
        );
    }

    /// A `direct`-download mod with a forgecdn URL and no md5 must produce ONE
    /// file in `files` awaiting CF sha1 resolution (sha1="", md5=None,
    /// version_id == the file id, url == the forgecdn url). It must NOT appear
    /// in `unresolvable` and must NOT have MissingChecksum.
    #[test]
    fn forgecdn_direct_no_md5_awaits_cf_resolution() {
        let m = AtlMod {
            name: "JourneyMap".into(),
            file: "journeymap-1.18.2-5.8.5-forge.jar".into(),
            url: "https://edge.forgecdn.net/files/3820/040/journeymap-1.18.2-5.8.5-forge.jar"
                .into(),
            md5: String::new(), // no md5 in manifest
            filesize: 500_000.0,
            mod_type: "mods".into(),
            download: "direct".into(),
            client: true,
            optional: false,
            cf_project_id: 0,
            cf_file_id: 0,
            version: String::new(),
        };
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![m]));
        assert_eq!(
            s.files.len(),
            1,
            "forgecdn direct mod should be in files awaiting sha1, got unresolvable: {:?}",
            s.unresolvable
        );
        assert!(
            s.unresolvable.is_empty(),
            "must NOT be unresolvable yet; got: {:?}",
            s.unresolvable
        );
        let f = &s.files[0];
        assert_eq!(f.source, ModSource::Atlauncher);
        assert_eq!(f.sha1, "", "sha1 must be empty (awaiting CF resolution)");
        assert!(f.md5.is_none(), "md5 must be None (not an md5 file)");
        assert_eq!(
            f.version_id, "3820040",
            "version_id must encode the CF file id"
        );
        assert_eq!(
            f.url, "https://edge.forgecdn.net/files/3820/040/journeymap-1.18.2-5.8.5-forge.jar",
            "url must be the forgecdn url (kept as manual fallback)"
        );
    }

    /// A `direct`-download mod to a NON-forgecdn host with empty md5 must still
    /// produce `MissingChecksum` (unchanged behavior — the forgecdn branch is
    /// only for forgecdn hosts).
    #[test]
    fn direct_no_md5_non_forgecdn_still_missing_checksum() {
        let m = AtlMod {
            name: "SomeMod".into(),
            file: "somemod.jar".into(),
            url: "https://example.com/somemod.jar".into(),
            md5: String::new(),
            filesize: 100.0,
            mod_type: "mods".into(),
            download: "direct".into(),
            client: true,
            optional: false,
            cf_project_id: 0,
            cf_file_id: 0,
            version: String::new(),
        };
        let s = map_configs("Pack", "1.0", CDN, &forge_configs(vec![m]));
        assert_eq!(s.files.len(), 0);
        assert_eq!(s.unresolvable.len(), 1);
        assert!(
            matches!(
                s.unresolvable[0].reason,
                UnresolvableReason::MissingChecksum
            ),
            "non-forgecdn host with no md5 must be MissingChecksum, got: {:?}",
            s.unresolvable[0].reason
        );
    }

    /// `resolve_url` unit tests.
    #[test]
    fn resolve_url_absolute_passthrough() {
        assert_eq!(
            resolve_url("https://cdn.example.com/base", "https://other.com/file.jar"),
            "https://other.com/file.jar"
        );
        assert_eq!(
            resolve_url(
                "https://cdn.example.com/base",
                "http://old.example.com/file.jar"
            ),
            "http://old.example.com/file.jar"
        );
    }

    #[test]
    fn resolve_url_empty_passthrough() {
        assert_eq!(resolve_url("https://cdn.example.com/base", ""), "");
    }

    #[test]
    fn resolve_url_relative_prepends_base() {
        assert_eq!(
            resolve_url(
                "https://download.nodecdn.net/containers/atl",
                "packs/X/files/mods/a.jar"
            ),
            "https://download.nodecdn.net/containers/atl/packs/X/files/mods/a.jar"
        );
        // Trailing slash on base is trimmed.
        assert_eq!(
            resolve_url(
                "https://download.nodecdn.net/containers/atl/",
                "packs/X/files/mods/a.jar"
            ),
            "https://download.nodecdn.net/containers/atl/packs/X/files/mods/a.jar"
        );
        // Leading slash on relative path is stripped.
        assert_eq!(
            resolve_url(
                "https://download.nodecdn.net/containers/atl",
                "/packs/X/files/mods/a.jar"
            ),
            "https://download.nodecdn.net/containers/atl/packs/X/files/mods/a.jar"
        );
    }
}
