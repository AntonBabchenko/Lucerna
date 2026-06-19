//! `ready` status check + effective-version-id derivation.
//!
//! "Ready" means: the effective version JAR exists on disk. If the JAR
//! exists but some libraries are missing, `install_and_launch` will
//! re-fetch them idempotently — no harm; we don't need to verify libs
//! here. The cost of a wrong "ready=true" is one extra round of HTTP
//! 304s on launch. The cost of slow status checks is jank in the
//! dropdown.

use crate::instances::schema::{InstanceFile, LoaderKind};
use crate::versions::loaders::{parse_synth_id, synth_id, Loader};
use std::path::Path;

/// What version id this instance launches as.
///
/// - Vanilla → `mc_version` (e.g. `"1.20.4"`)
/// - Fabric/Quilt → `<loader>-loader-<loader_version>-<mc_version>` (e.g.
///   `"fabric-loader-0.16.5-1.20.4"`) via `versions::loaders::synth_id`.
///
/// Returns `None` when:
/// - `mc_version` is empty (fresh-install state — UI prompts user)
/// - loader is Fabric/Quilt but `loader_version` is `None` (transient
///   pre-set state; the UI will set the latest stable on switch)
pub fn effective_version_id(instance: &InstanceFile) -> Option<String> {
    if instance.mc_version.is_empty() {
        return None;
    }
    match instance.loader {
        LoaderKind::Vanilla => Some(instance.mc_version.clone()),
        LoaderKind::Fabric => instance
            .loader_version
            .as_deref()
            .map(|lv| synth_id(Loader::Fabric, lv, &instance.mc_version)),
        LoaderKind::Quilt => instance
            .loader_version
            .as_deref()
            .map(|lv| synth_id(Loader::Quilt, lv, &instance.mc_version)),
        LoaderKind::Forge => instance
            .loader_version
            .as_deref()
            .map(|lv| synth_id(Loader::Forge, lv, &instance.mc_version)),
        LoaderKind::NeoForge => instance
            .loader_version
            .as_deref()
            .map(|lv| synth_id(Loader::NeoForge, lv, &instance.mc_version)),
    }
}

/// True iff the effective version's install artefacts exist on disk:
/// the profile JSON for the effective id AND the vanilla MC client jar
/// that launch will read. Two cheap stat calls. If `effective_version_id`
/// is None, returns false.
///
/// Two checks because:
/// - The profile JSON proves install_version was called for THIS exact
///   (mc, loader, loader_version) combo — catches the case where the
///   user switches loader version via Manage without clicking Install
///   again. Without this check, an unrelated vanilla install of the
///   parent MC would falsely satisfy the jar-only check.
/// - The client jar lives at `versions/<parent_mc>/<parent_mc>.jar`
///   even for synth ids (Fabric/Quilt/Forge/NeoForge) per the v0.4.1
///   invariant in install.rs and spawn.rs (vanilla MC jar is never
///   duplicated under a synth dir). Resolve through parse_synth_id so
///   this check matches what launch::spawn looks for.
pub fn ready_status(versions_dir: &Path, instance: &InstanceFile) -> bool {
    let Some(effective_id) = effective_version_id(instance) else {
        return false;
    };
    let profile_json = versions_dir
        .join(&effective_id)
        .join(format!("{effective_id}.json"));
    if !profile_json.is_file() {
        return false;
    }
    let client_jar_id = parse_synth_id(&effective_id)
        .map(|(_loader, _lv, mc)| mc)
        .unwrap_or_else(|| effective_id.clone());
    versions_dir
        .join(&client_jar_id)
        .join(format!("{client_jar_id}.jar"))
        .is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make(mc: &str, loader: LoaderKind, lv: Option<&str>) -> InstanceFile {
        InstanceFile {
            id: "x".into(),
            name: "x".into(),
            mc_version: mc.into(),
            loader,
            loader_version: lv.map(String::from),
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 0.0,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from: None,
            created_from_server: None,
            handled_log_sig: None,
        }
    }

    #[test]
    fn vanilla_effective_id_is_mc_version() {
        let inst = make("1.20.4", LoaderKind::Vanilla, None);
        assert_eq!(effective_version_id(&inst), Some("1.20.4".into()));
    }

    #[test]
    fn fabric_effective_id_uses_synth_format() {
        let inst = make("1.20.4", LoaderKind::Fabric, Some("0.16.5"));
        assert_eq!(
            effective_version_id(&inst),
            Some("fabric-loader-0.16.5-1.20.4".into())
        );
    }

    #[test]
    fn quilt_effective_id_uses_synth_format() {
        let inst = make("1.21", LoaderKind::Quilt, Some("0.27.1"));
        assert_eq!(
            effective_version_id(&inst),
            Some("quilt-loader-0.27.1-1.21".into())
        );
    }

    #[test]
    fn empty_mc_version_gives_none() {
        let inst = make("", LoaderKind::Vanilla, None);
        assert_eq!(effective_version_id(&inst), None);
    }

    #[test]
    fn fabric_without_loader_version_gives_none() {
        let inst = make("1.20.4", LoaderKind::Fabric, None);
        assert_eq!(effective_version_id(&inst), None);
    }

    #[test]
    fn ready_false_when_jar_missing() {
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Vanilla, None);
        assert!(!ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_true_when_profile_json_and_jar_present() {
        // Vanilla: profile json at versions/<mc>/<mc>.json and jar at
        // versions/<mc>/<mc>.jar — both required.
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Vanilla, None);
        let mc_dir = dir.path().join("1.20.4");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("1.20.4.json"), b"{}").unwrap();
        std::fs::write(mc_dir.join("1.20.4.jar"), b"fake").unwrap();
        assert!(ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_false_when_only_jar_no_profile_json() {
        // Vanilla MC parent jar without the version's own profile json
        // is not "ready" — the launch path needs the profile to read
        // libraries, javaVersion, etc.
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Vanilla, None);
        let mc_dir = dir.path().join("1.20.4");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("1.20.4.jar"), b"fake").unwrap();
        assert!(!ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_true_for_fabric_when_synth_json_and_parent_mc_jar_present() {
        // Synth instances need BOTH: the synth profile JSON at
        // versions/<synth>/<synth>.json (proves install_version was called
        // for THIS exact loader+MC combo) AND the parent MC client jar at
        // versions/<parent_mc>/<parent_mc>.jar (per v0.4.1 invariant —
        // never duplicated under the synth dir).
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Fabric, Some("0.16.5"));
        let synth_dir = dir.path().join("fabric-loader-0.16.5-1.20.4");
        std::fs::create_dir_all(&synth_dir).unwrap();
        std::fs::write(synth_dir.join("fabric-loader-0.16.5-1.20.4.json"), b"{}").unwrap();
        let mc_dir = dir.path().join("1.20.4");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("1.20.4.jar"), b"fake").unwrap();
        assert!(ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_false_for_fabric_when_synth_json_missing_even_if_parent_mc_jar_present() {
        // The common breakage: user changes loader version (or creates
        // a fabric instance with the same MC as an existing vanilla),
        // never clicks Install. The parent MC jar is present from the
        // unrelated install, but the synth profile JSON for THIS combo
        // was never written — must read as not ready.
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Fabric, Some("0.16.5"));
        let mc_dir = dir.path().join("1.20.4");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("1.20.4.jar"), b"fake").unwrap();
        assert!(!ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_false_for_fabric_when_synth_jar_only_present() {
        // Defensive: even if a leftover synth-named jar exists (from
        // pre-v0.4.1 installs that duplicated the vanilla jar), the
        // resolution must follow the parent-mc path, so without the
        // parent jar the instance is not ready.
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Fabric, Some("0.16.5"));
        let synth_dir = dir.path().join("fabric-loader-0.16.5-1.20.4");
        std::fs::create_dir_all(&synth_dir).unwrap();
        std::fs::write(synth_dir.join("fabric-loader-0.16.5-1.20.4.json"), b"{}").unwrap();
        std::fs::write(synth_dir.join("fabric-loader-0.16.5-1.20.4.jar"), b"fake").unwrap();
        assert!(!ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_false_for_empty_mc_version_even_if_random_jars_present() {
        let dir = tempdir().unwrap();
        let inst = make("", LoaderKind::Vanilla, None);
        std::fs::create_dir_all(dir.path().join("1.20.4")).unwrap();
        std::fs::write(dir.path().join("1.20.4/1.20.4.jar"), b"fake").unwrap();
        assert!(!ready_status(dir.path(), &inst));
    }

    #[test]
    fn neoforge_effective_id_uses_synth_format() {
        let inst = make("1.20.4", LoaderKind::NeoForge, Some("20.4.245"));
        assert_eq!(
            effective_version_id(&inst),
            Some("neoforge-20.4.245-1.20.4".into())
        );
    }

    #[test]
    fn neoforge_without_loader_version_gives_none() {
        let inst = make("1.20.4", LoaderKind::NeoForge, None);
        assert_eq!(effective_version_id(&inst), None);
    }
}
