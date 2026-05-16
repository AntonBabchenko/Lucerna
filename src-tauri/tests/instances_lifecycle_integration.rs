//! Lifecycle of instances at the `instances::*` API level. Uses a
//! tempdir + manual filesystem prep — does NOT need a Tauri AppHandle
//! because we exercise the lower-level functions directly through
//! `store`/`scan`/`status`/`migrate`. The AppHandle-wrapping orchestrators
//! are exercised by manual e2e in Task 21.

use ftlauncher_lib::instances::ids::new_id;
use ftlauncher_lib::instances::scan::list_all;
use ftlauncher_lib::instances::schema::{AppFile, InstanceFile, LoaderKind};
use ftlauncher_lib::instances::status::ready_status;
use ftlauncher_lib::instances::store::{
    read_app_json, read_instance_json, write_app_json, write_instance_json,
};
use tempfile::tempdir;

fn make(id: &str, name: &str, mc: &str, created_unix_ms: f64) -> InstanceFile {
    InstanceFile {
        version: 1,
        id: id.into(),
        name: name.into(),
        mc_version: mc.into(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms,
    }
}

#[test]
fn create_set_active_set_version_set_memory_then_delete_roundtrip() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let instances = root.join("instances");
    let versions = root.join("versions");
    std::fs::create_dir_all(&versions).unwrap();

    // 1. Create two instances.
    let id_a = new_id();
    let id_b = new_id();
    write_instance_json(
        &instances.join(&id_a).join("instance.json"),
        &make(&id_a, "Alpha", "1.20.4", 1000.0),
    )
    .unwrap();
    write_instance_json(
        &instances.join(&id_b).join("instance.json"),
        &make(&id_b, "Beta", "1.21", 2000.0),
    )
    .unwrap();

    // 2. Scan should return both, oldest first.
    let scan = list_all(&instances);
    assert_eq!(scan.len(), 2);
    assert_eq!(scan[0].id, id_a);
    assert_eq!(scan[1].id, id_b);

    // 3. Set active to B (mutate app.json directly — simulates set_active_instance).
    let app_path = root.join("app.json");
    write_app_json(
        &app_path,
        &AppFile {
            version: 1,
            active_instance: Some(id_b.clone()),
        },
    )
    .unwrap();
    let app_state = read_app_json(&app_path).unwrap();
    assert_eq!(app_state.active_instance, Some(id_b.clone()));

    // 4. Set version on A: mc_version "1.20.4" → "1.20.2" (simulates set_instance_version).
    let mut a = read_instance_json(&instances.join(&id_a).join("instance.json")).unwrap();
    a.mc_version = "1.20.2".into();
    write_instance_json(&instances.join(&id_a).join("instance.json"), &a).unwrap();
    let a_after = read_instance_json(&instances.join(&id_a).join("instance.json")).unwrap();
    assert_eq!(a_after.mc_version, "1.20.2");

    // 5. Set memory on A: 2048 → 4096 (simulates set_instance_memory).
    let mut a = read_instance_json(&instances.join(&id_a).join("instance.json")).unwrap();
    a.max_heap_mb = 4096;
    write_instance_json(&instances.join(&id_a).join("instance.json"), &a).unwrap();
    let a_after = read_instance_json(&instances.join(&id_a).join("instance.json")).unwrap();
    assert_eq!(a_after.max_heap_mb, 4096);

    // 6. Ready status — neither has a JAR on disk yet → false.
    assert!(!ready_status(&versions, &a_after));

    // 7. Place 1.20.2 JAR to flip A's ready → true.
    std::fs::create_dir_all(versions.join("1.20.2")).unwrap();
    std::fs::write(versions.join("1.20.2/1.20.2.jar"), b"fake").unwrap();
    assert!(ready_status(&versions, &a_after));

    // 8. Delete B (manual: rm -rf its dir, then update app.json).
    std::fs::remove_dir_all(instances.join(&id_b)).unwrap();
    let _app_state = read_app_json(&app_path).unwrap();
    // delete_instance would have auto-switched; here we simulate that:
    let remaining = list_all(&instances);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, id_a);
    let new_active = remaining.first().map(|i| i.id.clone());
    write_app_json(
        &app_path,
        &AppFile {
            version: 1,
            active_instance: new_active.clone(),
        },
    )
    .unwrap();
    let app_state = read_app_json(&app_path).unwrap();
    assert_eq!(app_state.active_instance, Some(id_a));
    let _ = app_state; // silence warning
}

#[test]
fn corrupted_instance_skipped_but_others_visible() {
    let dir = tempdir().unwrap();
    let instances = dir.path().join("instances");
    let good_id = new_id();
    write_instance_json(
        &instances.join(&good_id).join("instance.json"),
        &make(&good_id, "Good", "1.20.4", 1000.0),
    )
    .unwrap();
    let bad = instances.join("corrupt-uuid").join("instance.json");
    std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
    std::fs::write(&bad, "{ not json").unwrap();

    let scan = list_all(&instances);
    assert_eq!(scan.len(), 1);
    assert_eq!(scan[0].name, "Good");
}
