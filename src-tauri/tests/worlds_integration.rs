//! End-to-end through the worlds module's public API. Builds a real
//! `tauri::AppHandle` fixture using the existing test pattern (see
//! `src-tauri/tests/multi_instance_isolation_integration.rs` for the
//! template).
//!
//! Note: these tests do NOT go through the Tauri command layer
//! (which would require a Builder fixture); they call the public
//! crate functions directly, which is the same path the commands
//! delegate to.

use lucerna_lib::worlds::{self, RestoreMode};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Spin up an isolated app-data dir + a single fabricated instance
/// with the given world folders pre-populated.
fn make_fixture(instance_id: &str, world_files: &[(&str, &[(&str, &[u8])])]) -> (TempDir, PathBuf) {
    let td = TempDir::new().unwrap();
    let inst_dir = td.path().join("instances").join(instance_id);
    let saves_dir = inst_dir.join(".minecraft").join("saves");
    fs::create_dir_all(&saves_dir).unwrap();
    for (world_name, files) in world_files {
        let world_dir = saves_dir.join(world_name);
        fs::create_dir_all(&world_dir).unwrap();
        for (rel, bytes) in *files {
            let p = world_dir.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, *bytes).unwrap();
        }
    }
    (td, inst_dir)
}

#[tokio::test]
async fn end_to_end_backup_then_restore_replace_round_trip() {
    use lucerna_lib::worlds::zip as wzip;

    let (_td, inst_dir) = make_fixture("inst-A", &[("World1", &[("level.dat", b"\x01\x02")])]);
    let saves_dir = inst_dir.join(".minecraft").join("saves");
    let world_dir = saves_dir.join("World1");
    let backups_dir = inst_dir.join("backups").join("World1");
    fs::create_dir_all(&backups_dir).unwrap();

    // Backup the world manually using the same zip_dir the production
    // backup_world wraps.
    let backup_zip = backups_dir.join("2026-05-24T10-00-00.zip");
    wzip::zip_dir(&world_dir, &backup_zip, "World1").unwrap();

    // Mutate the live world.
    fs::write(world_dir.join("level.dat"), b"MUTATED").unwrap();
    assert_eq!(fs::read(world_dir.join("level.dat")).unwrap(), b"MUTATED");

    // Restore via the public restore_backup fn (Replace mode).
    let r = worlds::restore::restore_backup_at_saves(
        &saves_dir,
        &backups_dir,
        &backup_zip,
        "World1",
        RestoreMode::Replace,
    )
    .await
    .unwrap();
    assert_eq!(r.final_folder_name, "World1");

    // World is back to the pre-mutation state.
    assert_eq!(fs::read(world_dir.join("level.dat")).unwrap(), b"\x01\x02");

    // Pre-restore zip captured the mutation. On the success path the snapshot
    // IS written — it is only skipped when the backup never verified.
    let pre: Vec<_> = fs::read_dir(&backups_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("pre-restore-"))
        .collect();
    assert_eq!(pre.len(), 1);

    // The reorder's guarantee, asserted where a caller would see it: a restore
    // leaves neither a staging directory nor a parked world behind. Both are
    // dot-prefixed, so no launcher listing would ever show them.
    let leftovers: Vec<_> = fs::read_dir(&saves_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(".tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "restore left staging behind: {leftovers:?}"
    );
}

#[tokio::test]
async fn end_to_end_backup_then_restore_as_copy_keeps_original() {
    use lucerna_lib::worlds::zip as wzip;

    let (_td, inst_dir) = make_fixture("inst-B", &[("Survival", &[("level.dat", b"v1")])]);
    let saves_dir = inst_dir.join(".minecraft").join("saves");
    let backups_dir = inst_dir.join("backups").join("Survival");
    fs::create_dir_all(&backups_dir).unwrap();

    let backup_zip = backups_dir.join("2026-05-24T11-00-00.zip");
    wzip::zip_dir(&saves_dir.join("Survival"), &backup_zip, "Survival").unwrap();

    fs::write(saves_dir.join("Survival").join("level.dat"), b"v2").unwrap();

    let r = worlds::restore::restore_backup_at_saves(
        &saves_dir,
        &backups_dir,
        &backup_zip,
        "Survival",
        RestoreMode::AsCopy,
    )
    .await
    .unwrap();
    assert_eq!(r.final_folder_name, "Survival (restored)");

    // Original kept v2, the copy has v1.
    assert_eq!(
        fs::read(saves_dir.join("Survival").join("level.dat")).unwrap(),
        b"v2"
    );
    assert_eq!(
        fs::read(saves_dir.join("Survival (restored)").join("level.dat")).unwrap(),
        b"v1"
    );
}
