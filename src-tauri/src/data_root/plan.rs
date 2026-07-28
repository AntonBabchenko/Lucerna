//! Classify a user-picked directory into a data-location change plan.
//!
//! The Storage panel used to append the `LucernaData` subfolder in the
//! frontend and the backend migrated into whatever path arrived. Nothing
//! recognized "this folder already IS a Lucerna data root", so picking an
//! existing root nested a fresh empty root inside it
//! (`LucernaData\LucernaData`) and abandoned the real data. Planning now
//! lives here, behind one pure function; the frontend builds no paths at all.

use std::path::{Path, PathBuf};

/// Name of the dedicated subfolder created inside a user-picked container.
/// Human-readable (not the `com.lucerna.app` identifier) and distinct from
/// the launcher's install folder (`Lucerna`). Single source of truth — the
/// frontend no longer assembles paths.
pub const DATA_SUBFOLDER: &str = "LucernaData";

/// A planned data-location change. `Adopt` points the redirect at an existing
/// root without moving anything; `Migrate` is the classic copy→verify→delete
/// flow into the contained path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanKind {
    Adopt(PathBuf),
    Migrate(PathBuf),
}

/// True when `dir` looks like a Lucerna data root: an `instances/` directory
/// plus an `app.json` file whose content parses as a JSON object.
///
/// Shape-based on purpose — there is no marker file, and requiring
/// `versions/` would reject legitimately adoptable fresh roots
/// (`instances::migrate::migrate_or_seed` guarantees only `app.json` +
/// `instances/`; `versions/` appears on the first version install). The JSON
/// check is schema-free so roots written by any past launcher version keep
/// detecting.
pub fn is_data_root(dir: &Path) -> bool {
    if !dir.join("instances").is_dir() {
        return false;
    }
    let app_json = dir.join("app.json");
    if !app_json.is_file() {
        return false;
    }
    match std::fs::read_to_string(&app_json) {
        Ok(raw) => matches!(
            serde_json::from_str::<serde_json::Value>(&raw),
            Ok(serde_json::Value::Object(_))
        ),
        Err(_) => false,
    }
}

/// True when `path`'s last component equals [`DATA_SUBFOLDER`], compared
/// case-insensitively on every platform: the name is launcher-owned, and this
/// rule only suppresses a cosmetic re-append — matching loosely can never
/// lose data (a migrate target must still be empty to be accepted).
fn ends_with_data_subfolder(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(DATA_SUBFOLDER))
        .unwrap_or(false)
}

/// Pure classification of a picked directory. `is_root` is injected so the
/// decision table is testable without a filesystem.
///
/// Rule order matters:
/// 1. The picked dir itself is a root → adopt it (covers picking the root
///    directly, whatever it is named).
/// 2. Its `LucernaData` child is a root → adopt the child (covers picking the
///    container — including a container itself named `LucernaData`, which is
///    why this runs before rule 3).
/// 3. The picked dir is already named `LucernaData` → migrate into it
///    verbatim (never double the suffix).
/// 4. Otherwise → migrate into `picked/LucernaData` (classic flow).
pub fn plan_change(picked: &Path, is_root: &dyn Fn(&Path) -> bool) -> PlanKind {
    if is_root(picked) {
        return PlanKind::Adopt(picked.to_path_buf());
    }
    let child = picked.join(DATA_SUBFOLDER);
    if is_root(&child) {
        return PlanKind::Adopt(child);
    }
    if ends_with_data_subfolder(picked) {
        return PlanKind::Migrate(picked.to_path_buf());
    }
    PlanKind::Migrate(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed_root(dir: &Path) {
        std::fs::create_dir_all(dir.join("instances")).unwrap();
        std::fs::write(dir.join("app.json"), br#"{"active_instance":null}"#).unwrap();
    }

    #[test]
    fn detects_minimal_root_without_versions_dir() {
        let d = tempdir().unwrap();
        seed_root(d.path());
        assert!(is_data_root(d.path()));
    }

    #[test]
    fn rejects_dir_without_instances() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("app.json"), b"{}").unwrap();
        assert!(!is_data_root(d.path()));
    }

    #[test]
    fn rejects_dir_without_app_json() {
        let d = tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("instances")).unwrap();
        assert!(!is_data_root(d.path()));
    }

    #[test]
    fn rejects_app_json_that_is_not_a_json_object() {
        let d = tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("instances")).unwrap();
        for bad in [&b"[]"[..], &b"not json"[..], &b"42"[..]] {
            std::fs::write(d.path().join("app.json"), bad).unwrap();
            assert!(!is_data_root(d.path()), "accepted {bad:?}");
        }
    }

    #[test]
    fn rejects_instances_as_a_file() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("instances"), b"").unwrap();
        std::fs::write(d.path().join("app.json"), b"{}").unwrap();
        assert!(!is_data_root(d.path()));
    }

    #[test]
    fn rejects_missing_dir() {
        let d = tempdir().unwrap();
        assert!(!is_data_root(&d.path().join("nope")));
    }

    // ---- plan_change decision table (pure — injected is_root) ----

    fn no_roots(_: &Path) -> bool {
        false
    }

    #[test]
    fn picked_root_is_adopted() {
        let picked = PathBuf::from("/x/MyData");
        let is_root = |p: &Path| p == Path::new("/x/MyData");
        assert_eq!(
            plan_change(&picked, &is_root),
            PlanKind::Adopt(PathBuf::from("/x/MyData"))
        );
    }

    #[test]
    fn container_with_root_child_adopts_the_child() {
        let picked = PathBuf::from("/x/Container");
        let child = picked.join(DATA_SUBFOLDER);
        let is_root = move |p: &Path| p == child.as_path();
        assert_eq!(
            plan_change(&picked, &is_root),
            PlanKind::Adopt(PathBuf::from("/x/Container").join(DATA_SUBFOLDER))
        );
    }

    #[test]
    fn container_named_lucernadata_with_root_child_still_adopts_the_child() {
        // Rule 2 must run before rule 3: outer plain folder happens to be
        // named LucernaData, inner LucernaData is the real root (the exact
        // doubled-path shape the incident produced).
        let picked = PathBuf::from("/x/LucernaData");
        let child = picked.join(DATA_SUBFOLDER);
        let is_root = move |p: &Path| p == child.as_path();
        assert_eq!(
            plan_change(&picked, &is_root),
            PlanKind::Adopt(PathBuf::from("/x/LucernaData").join(DATA_SUBFOLDER))
        );
    }

    #[test]
    fn dir_named_lucernadata_migrates_verbatim_no_doubling() {
        for name in ["LucernaData", "lucernadata", "LUCERNADATA"] {
            let picked = PathBuf::from(format!("/x/{name}"));
            assert_eq!(
                plan_change(&picked, &no_roots),
                PlanKind::Migrate(picked.clone()),
                "doubled the subfolder for {name}"
            );
        }
    }

    #[test]
    fn plain_container_migrates_into_subfolder() {
        let picked = PathBuf::from("/x/Games");
        assert_eq!(
            plan_change(&picked, &no_roots),
            PlanKind::Migrate(PathBuf::from("/x/Games").join(DATA_SUBFOLDER))
        );
    }

    #[test]
    fn trailing_separator_is_tolerated() {
        // Path::file_name ignores trailing separators, so a picker result
        // like "C:\X\" still matches the no-doubling rule.
        let picked = PathBuf::from("/x/LucernaData/");
        assert_eq!(
            plan_change(&picked, &no_roots),
            PlanKind::Migrate(PathBuf::from("/x/LucernaData/"))
        );
    }
}
