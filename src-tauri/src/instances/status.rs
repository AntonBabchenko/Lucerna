//! `ready` status check + effective-version-id derivation.
//!
//! "Ready" means: the effective version JAR exists on disk. If the JAR
//! exists but some libraries are missing, `install_and_launch` will
//! re-fetch them idempotently — no harm; we don't need to verify libs
//! here. The cost of a wrong "ready=true" is one extra round of HTTP
//! 304s on launch. The cost of slow status checks is jank in the
//! dropdown.

use crate::instances::schema::{InstanceFile, LoaderKind};
use crate::versions::loaders::{synth_id, Loader};
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
    }
}

/// True iff the effective version's client JAR exists at
/// `<versions_dir>/<id>/<id>.jar`. Cheap (single stat call). If
/// `effective_version_id` is None, returns false.
pub fn ready_status(versions_dir: &Path, instance: &InstanceFile) -> bool {
    let Some(id) = effective_version_id(instance) else {
        return false;
    };
    versions_dir.join(&id).join(format!("{id}.jar")).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make(mc: &str, loader: LoaderKind, lv: Option<&str>) -> InstanceFile {
        InstanceFile {
            version: 1,
            id: "x".into(),
            name: "x".into(),
            mc_version: mc.into(),
            loader,
            loader_version: lv.map(String::from),
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 0.0,
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
    fn ready_true_when_jar_present() {
        let dir = tempdir().unwrap();
        let inst = make("1.20.4", LoaderKind::Vanilla, None);
        let jar_dir = dir.path().join("1.20.4");
        std::fs::create_dir_all(&jar_dir).unwrap();
        std::fs::write(jar_dir.join("1.20.4.jar"), b"fake").unwrap();
        assert!(ready_status(dir.path(), &inst));
    }

    #[test]
    fn ready_false_for_empty_mc_version_even_if_random_jars_present() {
        let dir = tempdir().unwrap();
        let inst = make("", LoaderKind::Vanilla, None);
        std::fs::create_dir_all(dir.path().join("1.20.4")).unwrap();
        std::fs::write(dir.path().join("1.20.4/1.20.4.jar"), b"fake").unwrap();
        assert!(!ready_status(dir.path(), &inst));
    }
}
