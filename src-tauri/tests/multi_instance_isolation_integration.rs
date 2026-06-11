//! Two instances must have disjoint `.minecraft/` directories. A mod
//! placed in instance A is invisible from instance B's filesystem view.
//! Does not actually launch Minecraft.

use lucerna_lib::instances::ids::new_id;
use lucerna_lib::instances::schema::{InstanceFile, LoaderKind};
use lucerna_lib::instances::store::write_instance_json;
use tempfile::tempdir;

fn make(id: &str, name: &str) -> InstanceFile {
    InstanceFile {
        id: id.into(),
        name: name.into(),
        mc_version: "1.20.4".into(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
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
    }
}

#[test]
fn mods_in_instance_a_are_invisible_from_instance_b() {
    let dir = tempdir().unwrap();
    let instances = dir.path().join("instances");
    let id_a = new_id();
    let id_b = new_id();

    write_instance_json(
        &instances.join(&id_a).join("instance.json"),
        &make(&id_a, "Alpha"),
    )
    .unwrap();
    write_instance_json(
        &instances.join(&id_b).join("instance.json"),
        &make(&id_b, "Beta"),
    )
    .unwrap();

    let mods_a = instances.join(&id_a).join(".minecraft").join("mods");
    let mods_b = instances.join(&id_b).join(".minecraft").join("mods");
    std::fs::create_dir_all(&mods_a).unwrap();
    std::fs::create_dir_all(&mods_b).unwrap();

    std::fs::write(mods_a.join("sodium.jar"), b"fake jar").unwrap();

    let b_mods: Vec<_> = std::fs::read_dir(&mods_b)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    assert!(
        b_mods.is_empty(),
        "instance B should see no mods, got {b_mods:?}"
    );

    let a_mods: Vec<_> = std::fs::read_dir(&mods_a)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(a_mods, vec!["sodium.jar".to_string()]);
}

#[test]
fn natives_and_logs_dirs_are_also_per_instance() {
    let dir = tempdir().unwrap();
    let instances = dir.path().join("instances");
    let id_a = new_id();
    let id_b = new_id();

    write_instance_json(
        &instances.join(&id_a).join("instance.json"),
        &make(&id_a, "Alpha"),
    )
    .unwrap();
    write_instance_json(
        &instances.join(&id_b).join("instance.json"),
        &make(&id_b, "Beta"),
    )
    .unwrap();

    // Simulate what launch::start would mkdir.
    std::fs::create_dir_all(instances.join(&id_a).join("natives")).unwrap();
    std::fs::create_dir_all(instances.join(&id_a).join("logs")).unwrap();
    std::fs::write(instances.join(&id_a).join("logs/run.log"), b"alpha log").unwrap();

    assert!(!instances.join(&id_b).join("natives").exists());
    assert!(!instances.join(&id_b).join("logs/run.log").exists());
}
