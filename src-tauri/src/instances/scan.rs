//! Scan `<app_data_dir>/instances/` and return every parseable
//! `instance.json`. Corrupted entries (unparseable JSON, missing file)
//! are logged with `eprintln!` and skipped — the UI sees a smaller list
//! rather than an error.
//!
//! Filesystem is the source of truth for "which instances exist". No
//! index file. `app.json.active_instance` is a pointer into this list;
//! a stale pointer is repaired in `mod::get_active_instance`.

use crate::instances::schema::InstanceFile;
use crate::instances::store::read_instance_json;
use std::path::Path;

/// Return every parseable `instance.json` under `instances_dir`, sorted
/// by `created_unix_ms` ascending (oldest first — also the order shown
/// in the UI dropdown).
///
/// Missing `instances_dir` → empty vec (fresh install, no instances
/// folder created yet). I/O errors enumerating the dir → empty vec
/// with `eprintln!` warning.
pub fn list_all(instances_dir: &Path) -> Vec<InstanceFile> {
    let entries = match std::fs::read_dir(instances_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!(
                "[instances::scan] cannot read {}: {e}",
                instances_dir.display()
            );
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let json_path = entry.path().join("instance.json");
        match read_instance_json(&json_path) {
            Ok(file) => out.push(file),
            Err(e) => {
                eprintln!("[instances::scan] skipping {}: {e}", json_path.display());
            }
        }
    }
    out.sort_by(|a, b| {
        a.created_unix_ms
            .partial_cmp(&b.created_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use crate::instances::store::write_instance_json;
    use tempfile::tempdir;

    fn make(id: &str, name: &str, created_unix_ms: f64) -> InstanceFile {
        InstanceFile {
            version: 1,
            id: id.into(),
            name: name.into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
        }
    }

    #[test]
    fn missing_instances_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let scan = list_all(&dir.path().join("instances"));
        assert!(scan.is_empty());
    }

    #[test]
    fn empty_instances_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let instances = dir.path().join("instances");
        std::fs::create_dir_all(&instances).unwrap();
        assert!(list_all(&instances).is_empty());
    }

    #[test]
    fn lists_two_instances_sorted_by_created_unix_ms_asc() {
        let dir = tempdir().unwrap();
        let instances = dir.path().join("instances");
        write_instance_json(
            &instances.join("aaaa").join("instance.json"),
            &make("aaaa", "Newer", 2000.0),
        )
        .unwrap();
        write_instance_json(
            &instances.join("bbbb").join("instance.json"),
            &make("bbbb", "Older", 1000.0),
        )
        .unwrap();
        let scan = list_all(&instances);
        assert_eq!(scan.len(), 2);
        assert_eq!(scan[0].name, "Older");
        assert_eq!(scan[1].name, "Newer");
    }

    #[test]
    fn corrupted_instance_json_is_skipped() {
        let dir = tempdir().unwrap();
        let instances = dir.path().join("instances");
        write_instance_json(
            &instances.join("good").join("instance.json"),
            &make("good", "Good", 1000.0),
        )
        .unwrap();
        let bad = instances.join("bad").join("instance.json");
        std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
        std::fs::write(&bad, "{ not valid json").unwrap();

        let scan = list_all(&instances);
        assert_eq!(scan.len(), 1);
        assert_eq!(scan[0].name, "Good");
    }

    #[test]
    fn directory_without_instance_json_is_skipped() {
        let dir = tempdir().unwrap();
        let instances = dir.path().join("instances");
        std::fs::create_dir_all(instances.join("orphan/.minecraft")).unwrap();
        // No instance.json in instances/orphan/.
        assert!(list_all(&instances).is_empty());
    }
}
