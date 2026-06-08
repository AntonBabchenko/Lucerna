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

fn loader_from(configs: &AtlConfigs) -> (LoaderKind, Option<String>) {
    match &configs.loader {
        Some(l) => {
            let kind = match l.loader_type.as_str() {
                "fabric" => LoaderKind::Fabric,
                "quilt" => LoaderKind::Quilt,
                "forge" => LoaderKind::Forge,
                "neoforge" => LoaderKind::NeoForge,
                _ => return (LoaderKind::Vanilla, None),
            };
            let ver = l
                .metadata
                .as_ref()
                .map(|m| m.version.clone())
                .filter(|v| !v.is_empty());
            (kind, ver)
        }
        None => (LoaderKind::Vanilla, None),
    }
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
pub fn map_configs(pack_name: &str, version_name: &str, c: &AtlConfigs) -> ModpackSummary {
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
            unresolvable.push(unres(
                UnresolvableReason::DistributionDisabled,
                &m.name,
                &m.url,
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        if m.md5.trim().is_empty() {
            unresolvable.push(unres(
                UnresolvableReason::MissingChecksum,
                &m.name,
                &m.url,
                &m.file,
                m.filesize,
                None,
            ));
            continue;
        }

        let host = url::Url::parse(&m.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
        if !host.as_deref().is_some_and(is_host_allowed) {
            unresolvable.push(unres(
                UnresolvableReason::HostNotAllowed,
                &m.name,
                &m.url,
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
            sha1: String::new(),
            url: m.url.clone(),
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
    use crate::mods::modpack::atl_api::{AtlConfigs, AtlLoader, AtlLoaderMeta, AtlMod};

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

    fn with_nodecdn<F: FnOnce() -> R, R>(f: F) -> R {
        let _g = crate::test_env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "download.nodecdn.net");
        let r = f();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        r
    }

    #[test]
    fn maps_loader_and_mc() {
        let s = map_configs("Pack", "1.0", &forge_configs(vec![]));
        assert_eq!(s.format, ModpackFormat::Atlauncher);
        assert_eq!(s.game_version, "1.12.2");
        assert_eq!(s.loader, LoaderKind::Forge);
        assert_eq!(s.loader_version.as_deref(), Some("14.23.5.2847"));
    }

    #[test]
    fn server_mod_carries_md5_and_empty_sha1() {
        let s = with_nodecdn(|| {
            map_configs(
                "Pack",
                "1.0",
                &forge_configs(vec![base_mod("jei", "server")]),
            )
        });
        assert_eq!(s.files.len(), 1);
        let f = &s.files[0];
        assert_eq!(f.source, ModSource::Atlauncher);
        assert_eq!(f.md5.as_deref(), Some("abc123"));
        assert!(f.sha1.is_empty(), "sha1 is resolved post-download");
        assert_eq!(f.install_path, "mods/jei.jar");
    }

    #[test]
    fn cf_mod_becomes_curseforge_placeholder() {
        let mut m = base_mod("ae2", "direct");
        m.cf_project_id = 238222;
        m.cf_file_id = 4499899;
        m.md5 = String::new();
        let s = map_configs("Pack", "1.0", &forge_configs(vec![m]));
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
        let s = map_configs("Pack", "1.0", &forge_configs(vec![m]));
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
        let s = map_configs("Pack", "1.0", &forge_configs(vec![m]));
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
        let s = map_configs("Pack", "1.0", &forge_configs(vec![m]));
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
        let s = with_nodecdn(|| map_configs("Pack", "1.0", &forge_configs(vec![opt, srv])));
        assert_eq!(s.files.len(), 0);
        assert_eq!(s.unresolvable.len(), 0);
    }
}
