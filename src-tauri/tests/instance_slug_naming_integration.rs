//! The readable-slug directory-naming contract at the instances API level:
//! reserve a directory → write `instance.json` with the RETURNED id → `scan`
//! reads it back, with the on-disk directory name equal to the id.
//!
//! Tempdir-based (no Tauri AppHandle), mirroring
//! `instances_lifecycle_integration.rs`. This guards the wiring contract that
//! path resolution depends on: the id persisted into `instance.json` must be
//! the reserved directory name, never the pre-reservation base slug.

use lucerna_lib::instances::scan::list_all;
use lucerna_lib::instances::schema::{InstanceFile, LoaderKind};
use lucerna_lib::instances::store::write_instance_json;
use lucerna_lib::naming::reserve_unique_dir;
use std::path::Path;
use tempfile::tempdir;

fn seed(instances: &Path, name: &str, created_unix_ms: f64) -> String {
    let (id, dir) = reserve_unique_dir(instances, name, None, "instance").unwrap();
    let inst = InstanceFile {
        id: id.clone(),
        uid: None,
        name: name.into(),
        mc_version: "1.20.4".into(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
        max_heap_mb: 2048,
        min_heap_mb: None,
        extra_jvm_args: String::new(),
        created_unix_ms,
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
    };
    // Persist using the RESERVED id (the wiring contract under test).
    write_instance_json(&dir.join("instance.json"), &inst).unwrap();
    id
}

#[test]
fn same_name_instances_get_distinct_readable_dirs_and_scan_sees_both() {
    let dir = tempdir().unwrap();
    let instances = dir.path().join("instances");
    let a = seed(&instances, "My Pack", 1000.0);
    let b = seed(&instances, "My Pack", 2000.0);
    assert_eq!(a, "My-Pack");
    assert_eq!(b, "My-Pack-2");

    let all = list_all(&instances);
    let ids: Vec<_> = all.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        ["My-Pack", "My-Pack-2"],
        "oldest-first by created_unix_ms"
    );

    // The invariant path resolution relies on: the id scan returns is exactly
    // the on-disk directory name.
    for i in &all {
        assert!(
            instances.join(&i.id).join("instance.json").is_file(),
            "directory name must equal the id ({})",
            i.id
        );
    }
}

#[test]
fn cyrillic_name_yields_a_readable_transliterated_directory() {
    // This test previously asserted the directory was `Мой-сервер`. Directory
    // names are ASCII now: the Windows JVM launcher decodes argv through the
    // system ANSI code page, so a name that code page cannot express reaches
    // java.exe as `?` and the game dies before Minecraft starts. Readability is
    // preserved by transliteration rather than by keeping the original script.
    let dir = tempdir().unwrap();
    let instances = dir.path().join("instances");
    let id = seed(&instances, "Мой сервер", 1000.0);
    assert_eq!(id, "Moi-server");
    assert!(instances.join(&id).join("instance.json").is_file());

    let all = list_all(&instances);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "Moi-server");
}

#[test]
fn cjk_name_with_a_latin_part_keeps_the_latin_part() {
    // The bug that started this: the pack directory was
    // `红石生电优化-Redstone-Survival-Optimiz`, and the JVM received the CJK
    // characters as `?`, which is an illegal Windows path character.
    let dir = tempdir().unwrap();
    let instances = dir.path().join("instances");
    let id = seed(&instances, "红石生电优化【Redstone Survival Optimization】", 1000.0);
    assert_eq!(id, "Redstone-Survival-Optimization");
    assert!(id.is_ascii());
    assert!(instances.join(&id).join("instance.json").is_file());
}
