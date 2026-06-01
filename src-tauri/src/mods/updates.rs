//! Standalone mod-update check: classify each installed user-mod
//! against the versions a platform currently lists for the instance's
//! Minecraft version + loader. Pure logic — no I/O — so the command
//! layer in `commands.rs` stays a thin orchestrator.

use serde::Serialize;
use specta::Type;

use crate::mods::installed::PackOrigin;
use crate::mods::platform::{InstalledMod, ModSource, ModVersion};

/// One installed user-mod's update-check result. One per *eligible*
/// mod — see [`eligible_identity`]; ineligible mods are absent.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModUpdateCheck {
    /// SHA-1 of the currently installed jar — identifies the row and is
    /// the handle `mods_update_one` uses to remove the old file.
    pub sha1: String,
    /// Display name from the registry.
    pub name: String,
    pub source: ModSource,
    pub project_id: String,
    pub current_version_id: String,
    pub current_version_number: Option<String>,
    pub state: ModUpdateState,
}

/// The per-mod classification.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModUpdateState {
    /// The installed version is the newest for this MC + loader.
    UpToDate,
    /// A newer version exists; `target` is the version to install.
    UpdateAvailable { target: ModVersion },
    /// Cannot determine — the installed version is not in the
    /// platform's current list, or the list is empty.
    Unknown,
    /// The platform query failed (network, missing CurseForge key,
    /// project delisted / 404). Set by `mods_check_updates` on a failed
    /// query — never produced by `classify_update`.
    CheckFailed { reason: String },
}

/// The pack name + the SHA-1s of its bundled mods. Feeds the Installed
/// tab's "from modpack" chip. `mod_shas` are lowercased.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PackOriginSummary {
    pub project_name: String,
    pub mod_shas: Vec<String>,
}

/// Classify `installed` against the platform's version list (which the
/// caller fetched, filtered to the instance's MC + loader, newest
/// first). Pure. Never returns `CheckFailed`.
pub fn classify_update(installed: &InstalledMod, versions: &[ModVersion]) -> ModUpdateState {
    let Some(current) = installed.version_id.as_deref() else {
        return ModUpdateState::Unknown;
    };
    let Some(newest) = versions.first() else {
        return ModUpdateState::Unknown;
    };
    if newest.version_id == current {
        return ModUpdateState::UpToDate;
    }
    // Only confident `newest` is an upgrade (not a downgrade) when the
    // installed version is itself somewhere in the list.
    if versions.iter().any(|v| v.version_id == current) {
        ModUpdateState::UpdateAvailable {
            target: newest.clone(),
        }
    } else {
        ModUpdateState::Unknown
    }
}

/// `true` iff `installed` is one of the modpack's bundled mods — its
/// SHA-1 matches a `mods/` entry in `pack_origin.files`. `false` when
/// the instance has no `pack_origin`.
pub fn is_pack_origin_mod(installed: &InstalledMod, pack_origin: Option<&PackOrigin>) -> bool {
    let Some(po) = pack_origin else {
        return false;
    };
    po.files.iter().any(|f| {
        f.install_path.starts_with("mods/") && f.sha1.eq_ignore_ascii_case(&installed.sha1)
    })
}

/// If `installed` is eligible for an update check, return its platform
/// identity `(source, project_id, version_id)`. `None` when the mod
/// lacks platform identity (a hand-dropped jar) or is a modpack-origin
/// mod. Replaces the spec's `is_eligible(...) -> bool`: returning the
/// identity lets the command layer use it without an `unwrap()`.
pub fn eligible_identity(
    installed: &InstalledMod,
    pack_origin: Option<&PackOrigin>,
) -> Option<(ModSource, String, String)> {
    if is_pack_origin_mod(installed, pack_origin) {
        return None;
    }
    match (
        installed.source,
        &installed.project_id,
        &installed.version_id,
    ) {
        (Some(source), Some(project_id), Some(version_id)) => {
            Some((source, project_id.clone(), version_id.clone()))
        }
        _ => None,
    }
}

/// Build the chip data for an instance's modpack origin: the pack name
/// and the lowercased SHA-1s of its bundled `mods/` files.
pub fn pack_origin_summary(pack_origin: &PackOrigin) -> PackOriginSummary {
    PackOriginSummary {
        project_name: pack_origin.project_name.clone(),
        mod_shas: pack_origin
            .files
            .iter()
            .filter(|f| f.install_path.starts_with("mods/"))
            .map(|f| f.sha1.to_ascii_lowercase())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::installed::{PackOrigin, PackOriginFile};
    use crate::mods::modpack::schema::EnvSupport;
    use crate::mods::platform::{LoaderKind, ModFile};

    fn installed_mod(
        sha1: &str,
        source: Option<ModSource>,
        project_id: Option<&str>,
        version_id: Option<&str>,
    ) -> InstalledMod {
        InstalledMod {
            filename: "mod.jar".into(),
            sha1: sha1.into(),
            source,
            project_id: project_id.map(String::from),
            version_id: version_id.map(String::from),
            name: "Mod".into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-05-22T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    fn version(version_id: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "p".into(),
            version_id: version_id.into(),
            name: "Mod".into(),
            version_number: version_id.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: format!("mod-{version_id}.jar"),
                url: "https://example/mod.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    fn pack_origin(files: &[(&str, &str)]) -> PackOrigin {
        PackOrigin {
            project_id: Some("pack".into()),
            source: ModSource::Modrinth,
            project_name: "Cool Pack".into(),
            version: "1.0".into(),
            files: files
                .iter()
                .map(|(sha, path)| PackOriginFile {
                    sha1: (*sha).into(),
                    name: "F".into(),
                    filename: "f.jar".into(),
                    install_path: (*path).into(),
                    url: "https://example/f.jar".into(),
                    size: 1.0,
                    project_id: "fp".into(),
                    version_id: "fv".into(),
                    env_client: EnvSupport::Required,
                    source: ModSource::Modrinth,
                })
                .collect(),
            missing_mods: vec![],
        }
    }

    #[test]
    fn classify_up_to_date_when_installed_is_newest() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), Some("v3"));
        let versions = vec![version("v3"), version("v2"), version("v1")];
        assert!(matches!(
            classify_update(&m, &versions),
            ModUpdateState::UpToDate
        ));
    }

    #[test]
    fn classify_update_available_when_newer_exists() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let versions = vec![version("v3"), version("v2"), version("v1")];
        match classify_update(&m, &versions) {
            ModUpdateState::UpdateAvailable { target } => assert_eq!(target.version_id, "v3"),
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
    }

    #[test]
    fn classify_unknown_when_installed_version_not_listed() {
        let m = installed_mod(
            "s1",
            Some(ModSource::Modrinth),
            Some("p"),
            Some("v-delisted"),
        );
        let versions = vec![version("v3"), version("v2")];
        assert!(matches!(
            classify_update(&m, &versions),
            ModUpdateState::Unknown
        ));
    }

    #[test]
    fn classify_unknown_when_version_list_empty() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        assert!(matches!(classify_update(&m, &[]), ModUpdateState::Unknown));
    }

    #[test]
    fn classify_unknown_when_installed_has_no_version_id() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), None);
        assert!(matches!(
            classify_update(&m, &[version("v1")]),
            ModUpdateState::Unknown
        ));
    }

    #[test]
    fn pack_origin_mod_detected_by_sha() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(is_pack_origin_mod(&m, Some(&po)));
    }

    #[test]
    fn user_mod_on_modpack_instance_is_not_pack_origin() {
        let m = installed_mod("bbb", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(!is_pack_origin_mod(&m, Some(&po)));
    }

    #[test]
    fn no_pack_origin_means_not_pack_origin() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        assert!(!is_pack_origin_mod(&m, None));
    }

    #[test]
    fn manual_jar_has_no_eligible_identity() {
        let m = installed_mod("s1", None, None, None);
        assert!(eligible_identity(&m, None).is_none());
    }

    #[test]
    fn pack_origin_mod_has_no_eligible_identity() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(eligible_identity(&m, Some(&po)).is_none());
    }

    #[test]
    fn user_browser_mod_is_eligible() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("proj"), Some("ver"));
        assert_eq!(
            eligible_identity(&m, None),
            Some((ModSource::Modrinth, "proj".to_string(), "ver".to_string()))
        );
    }

    #[test]
    fn pack_origin_summary_lists_only_mods_dir_shas_lowercased() {
        let po = pack_origin(&[("AAA", "mods/x.jar"), ("bbb", "resourcepacks/rp.zip")]);
        let s = pack_origin_summary(&po);
        assert_eq!(s.project_name, "Cool Pack");
        assert_eq!(s.mod_shas, vec!["aaa".to_string()]);
    }
}
