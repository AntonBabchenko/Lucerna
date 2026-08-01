//! End-to-end through the datapacks module's public API — library, world
//! link, and level.dat together, the same functions the Tauri commands
//! delegate to (see `worlds_integration.rs` for the template this mirrors).
//!
//! These tests do NOT go through the Tauri command layer (which would need a
//! Builder fixture); they call `lucerna_lib::datapacks::{library, world_link,
//! level_dat}` directly.
//!
//! ## Why there is no `test_env_lock` here
//!
//! `crate::test_env_lock()` is `#[cfg(test)] pub(crate)` in `lib.rs` — both
//! restrictions make it unreachable from an integration-test binary: the
//! `pub(crate)` visibility hides it from outside the crate, and `#[cfg(test)]`
//! means it is not even compiled into the `rlib` this binary links against (it
//! only exists in the lib crate's OWN unit-test build). `test_seam::scope` is
//! the one seam surface deliberately made `pub` for `tests/` binaries to use.
//!
//! That lock exists to serialize tests that race on the process-global
//! `LUCERNA_TEST_FORCE_LINK_FAILURE` seam (see `mods::store`'s and
//! `world_link`'s own unit tests). It is not needed here: every `tests/*.rs`
//! file compiles to its own separate OS process, and the lib crate's unit
//! tests (which DO exercise that seam) run in yet another, separate process.
//! Statics do not cross process boundaries, and this file never touches
//! `LUCERNA_TEST_FORCE_LINK_FAILURE` itself, so there is nothing for a lock to
//! serialize against here. `add_to_world_at` below uses whatever `LinkPolicy`
//! the real filesystem supports (a real hardlink, or `mods::store`'s
//! documented copy fallback) — this suite asserts on the resulting bytes, not
//! on which placement kind was chosen, so it passes either way.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use fastnbt::Value;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use lucerna_lib::datapacks::pack_meta::{self, PackKind};
use lucerna_lib::datapacks::{
    level_dat, library, library_dir_at, world_datapacks_dir_at, world_link, WorldPackState,
};
use lucerna_lib::mods::store::Placement;

/// Spin up an isolated instance tree: `<td>/instances/<instance_id>/` with
/// `.minecraft/saves/<world>/` pre-created for each named world. The library
/// dir (`<inst>/datapacks/`) is deliberately NOT created here — it is created
/// by the code under test on first install.
fn make_fixture(instance_id: &str, worlds: &[&str]) -> (TempDir, PathBuf) {
    let td = TempDir::new().unwrap();
    let inst_dir = td.path().join("instances").join(instance_id);
    let saves_dir = inst_dir.join(".minecraft").join("saves");
    fs::create_dir_all(&saves_dir).unwrap();
    for world in worlds {
        fs::create_dir_all(saves_dir.join(world)).unwrap();
    }
    (td, inst_dir)
}

/// Build a minimal, real datapack zip in memory: `pack.mcmeta` at the root
/// (with a `pack_format` and a description) plus one entry under `data/` —
/// the exact shape `pack_meta::classify` requires to call it `Datapack`
/// rather than `Neither`.
fn datapack_zip(pack_format: u32, description: &str) -> Vec<u8> {
    let mut zw = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zw.start_file("pack.mcmeta", opts).unwrap();
    zw.write_all(
        format!(r#"{{"pack":{{"pack_format":{pack_format},"description":"{description}"}}}}"#)
            .as_bytes(),
    )
    .unwrap();
    zw.start_file("data/vm/function/tick.mcfunction", opts)
        .unwrap();
    zw.write_all(b"say hi").unwrap();
    zw.finish().unwrap().into_inner()
}

#[tokio::test]
async fn install_add_to_two_worlds_and_the_bytes_are_shared() {
    let (_td, inst) = make_fixture("inst-share", &["Alpha", "Beta"]);
    let bytes = datapack_zip(48, "Vein Miner");

    library::install_named_at(&inst, "vm.zip", &bytes)
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Beta", "vm.zip")
        .await
        .unwrap();

    let alpha_path = world_datapacks_dir_at(&inst, "Alpha")
        .unwrap()
        .join("vm.zip");
    let beta_path = world_datapacks_dir_at(&inst, "Beta")
        .unwrap()
        .join("vm.zip");
    assert!(alpha_path.exists(), "Alpha's linked copy must exist");
    assert!(beta_path.exists(), "Beta's linked copy must exist");

    let alpha_bytes = fs::read(&alpha_path).unwrap();
    let beta_bytes = fs::read(&beta_path).unwrap();
    // Both worlds must see the exact original bytes, not just "some" bytes —
    // and therefore also identical to each other.
    assert_eq!(alpha_bytes, bytes);
    assert_eq!(beta_bytes, bytes);
    assert_eq!(pack_meta::classify(&alpha_bytes), PackKind::Datapack);
    assert_eq!(pack_meta::classify(&beta_bytes), PackKind::Datapack);
}

#[tokio::test]
async fn removing_from_one_world_leaves_the_other_intact() {
    let (_td, inst) = make_fixture("inst-remove-world", &["Alpha", "Beta"]);
    let bytes = datapack_zip(48, "Vein Miner");

    library::install_named_at(&inst, "vm.zip", &bytes)
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Beta", "vm.zip")
        .await
        .unwrap();

    world_link::remove_from_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();

    let alpha_path = world_datapacks_dir_at(&inst, "Alpha")
        .unwrap()
        .join("vm.zip");
    let beta_path = world_datapacks_dir_at(&inst, "Beta")
        .unwrap()
        .join("vm.zip");
    assert!(!alpha_path.exists(), "Alpha's name must be gone");
    assert!(beta_path.exists(), "Beta's file must be untouched");

    // Content, not just existence: a regression that "removed" Alpha's pack
    // via an in-place truncating write instead of `remove_file` would corrupt
    // the physical file every name (including Beta's) points at. A bare
    // existence check on Beta's path would still pass against a zeroed file;
    // a full byte comparison against the known-good original will not.
    let beta_bytes = fs::read(&beta_path).unwrap();
    assert_eq!(
        beta_bytes, bytes,
        "Beta's bytes must be untouched by removing Alpha's name"
    );
    assert_eq!(pack_meta::classify(&beta_bytes), PackKind::Datapack);
}

/// The invariant the whole design rests on: a world's `datapacks/` file is a
/// second NAME for the library's physical file, so writing through one name
/// changes the bytes every name sees. `removing_from_one_world_leaves_the_
/// other_intact` above does NOT prove this — both `add_to_world_at` calls
/// target fresh destination paths, so even a naive copy-per-world
/// implementation would produce two identical-but-independent files, and
/// deleting one NAME can never reveal whether the other name shares its
/// bytes.
///
/// This test proves it by writing through Alpha's world-side path directly —
/// exactly the in-place write `mods::store` forbids and every datapack write
/// path (`materialize`) is built to avoid — and then reading Beta's path
/// back.
///
/// The assertion is conditional on the `Placement` `add_to_world_at` actually
/// returned, and that is NOT a weaker test — it is the only correct one:
/// whether this dev/CI filesystem supports hardlinks is an environment fact
/// this test does not control (NTFS, ext4, most CI images: yes; some
/// container overlay filesystems, certain network shares: no).
///   * Both `Linked` → Alpha's and Beta's files are the SAME inode, so the
///     in-place write through Alpha's path must be visible through Beta's
///     path too. Seeing the OLD bytes here would mean the two names do not
///     actually share storage, i.e. the hardlink invariant this whole
///     feature is built on does not hold — that must fail the test.
///   * Either `Copied` → `mods::store::materialize` fell back to an
///     independent physical copy (the documented, safe degradation when
///     linking isn't available). Alpha and Beta are then genuinely separate
///     files, so a write through Alpha's path must NOT reach Beta's — seeing
///     the NEW bytes here would mean the "fallback" silently produced a
///     shared file anyway, which is its own bug.
/// Asserting unconditionally on one outcome would make this test flaky on
/// exactly the filesystems where linking isn't available — it would either
/// false-fail in that environment (asserting `Linked`-only behaviour) or
/// silently stop testing the invariant everywhere it usually matters
/// (asserting `Copied`-only behaviour). Branching on the real, observed
/// `Placement` is what keeps this deterministic everywhere while still
/// proving the right thing in each case.
#[tokio::test]
async fn writing_through_one_worlds_path_pins_the_shared_inode_invariant() {
    let (_td, inst) = make_fixture("inst-shared-inode", &["Alpha", "Beta"]);
    let original = datapack_zip(48, "Vein Miner");

    library::install_named_at(&inst, "vm.zip", &original)
        .await
        .unwrap();
    let alpha_placement = world_link::add_to_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();
    let beta_placement = world_link::add_to_world_at(&inst, "Beta", "vm.zip")
        .await
        .unwrap();

    let alpha_path = world_datapacks_dir_at(&inst, "Alpha")
        .unwrap()
        .join("vm.zip");
    let beta_path = world_datapacks_dir_at(&inst, "Beta")
        .unwrap()
        .join("vm.zip");

    // Simulate the exact write the design forbids: a raw, truncating write
    // straight through Alpha's world-side name, bypassing `mods::store`
    // entirely.
    let corrupted = datapack_zip(48, "Not Vein Miner Anymore");
    assert_ne!(
        corrupted, original,
        "the corrupting write must actually differ from the original"
    );
    fs::write(&alpha_path, &corrupted).unwrap();

    let beta_bytes_after = fs::read(&beta_path).unwrap();

    if alpha_placement == Placement::Linked && beta_placement == Placement::Linked {
        assert_eq!(
            beta_bytes_after, corrupted,
            "both adds linked the same inode, so an in-place write through \
             Alpha's name must be visible through Beta's name too"
        );
    } else {
        assert_eq!(
            beta_bytes_after, original,
            "at least one add fell back to an independent copy, so a write \
             through Alpha's name must not reach Beta's file"
        );
    }
}

#[tokio::test]
async fn removing_from_the_library_does_not_break_a_world_using_it() {
    let (_td, inst) = make_fixture("inst-remove-library", &["Alpha"]);
    let bytes = datapack_zip(48, "Vein Miner");

    library::install_named_at(&inst, "vm.zip", &bytes)
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();

    library::remove_at(&inst, "vm.zip").await.unwrap();

    let alpha_path = world_datapacks_dir_at(&inst, "Alpha")
        .unwrap()
        .join("vm.zip");
    assert!(alpha_path.exists(), "the world's own copy must survive");
    let alpha_bytes = fs::read(&alpha_path).unwrap();
    assert_eq!(alpha_bytes, bytes);
    assert_eq!(pack_meta::classify(&alpha_bytes), PackKind::Datapack);

    let listed = world_link::list_for_world_at(&inst, "Alpha", None)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filename, "vm.zip");
    assert!(
        !listed[0].in_library,
        "the library entry is gone; this pack now only lives in the world"
    );
    assert_eq!(listed[0].state, WorldPackState::Enabled);
}

#[tokio::test]
async fn a_disable_survives_a_level_dat_round_trip_alongside_unmodelled_tags() {
    let (_td, inst) = make_fixture("inst-level-dat-roundtrip", &["Alpha"]);
    let world_dir = inst.join(".minecraft").join("saves").join("Alpha");

    // Seed level.dat with tags nothing in this crate models: a Long, a
    // compound (GameRules), and an IntArray.
    let mut gamerules = HashMap::new();
    gamerules.insert("keepInventory".to_string(), Value::String("true".into()));
    let mut data = HashMap::new();
    data.insert("RandomSeed".to_string(), Value::Long(123_456_789));
    data.insert("GameRules".to_string(), Value::Compound(gamerules.clone()));
    data.insert(
        "SpawnPos".to_string(),
        Value::IntArray(fastnbt::IntArray::new(vec![10, 64, -10])),
    );
    let mut root_map = HashMap::new();
    root_map.insert("Data".to_string(), Value::Compound(data));
    let seed_root = Value::Compound(root_map);
    let seed_bytes = level_dat::serialize(&seed_root, level_dat::Framing::Gzip).unwrap();
    fs::write(world_dir.join("level.dat"), &seed_bytes).unwrap();

    let bytes = datapack_zip(48, "Vein Miner");
    library::install_named_at(&inst, "vm.zip", &bytes)
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();
    world_link::set_enabled_in_world_at(&inst, "Alpha", "vm.zip", false)
        .await
        .unwrap();

    let (after, _framing) = level_dat::read_at(&world_dir).unwrap();
    let Value::Compound(after_map) = &after else {
        panic!("level.dat root must still be a compound");
    };
    let Some(Value::Compound(after_data)) = after_map.get("Data") else {
        panic!("Data compound must still be present");
    };
    assert_eq!(
        after_data.get("RandomSeed"),
        Some(&Value::Long(123_456_789))
    );
    assert_eq!(
        after_data.get("GameRules"),
        Some(&Value::Compound(gamerules))
    );
    assert_eq!(
        after_data.get("SpawnPos"),
        Some(&Value::IntArray(fastnbt::IntArray::new(vec![10, 64, -10])))
    );

    let (enabled, disabled) = level_dat::lists(&after);
    assert!(enabled.is_empty());
    assert_eq!(disabled, vec!["file/vm.zip".to_string()]);
}

#[tokio::test]
async fn an_orphan_is_repairable_end_to_end() {
    let (_td, inst) = make_fixture("inst-orphan-repair", &["Alpha"]);
    let bytes = datapack_zip(48, "Vein Miner");

    library::install_named_at(&inst, "vm.zip", &bytes)
        .await
        .unwrap();
    world_link::add_to_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();

    let alpha_path = world_datapacks_dir_at(&inst, "Alpha")
        .unwrap()
        .join("vm.zip");
    assert!(alpha_path.exists());
    // Simulate the user deleting the world-side file directly in Explorer,
    // bypassing the datapacks module entirely.
    fs::remove_file(&alpha_path).unwrap();

    let listed = world_link::list_for_world_at(&inst, "Alpha", None)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filename, "vm.zip");
    assert_eq!(listed[0].state, WorldPackState::Orphaned);

    world_link::remove_from_world_at(&inst, "Alpha", "vm.zip")
        .await
        .unwrap();

    let world_dir = inst.join(".minecraft").join("saves").join("Alpha");
    let (root, _framing) = level_dat::read_at(&world_dir).unwrap();
    let (enabled, disabled) = level_dat::lists(&root);
    assert!(enabled.is_empty(), "the orphaned name must be cleared");
    assert!(disabled.is_empty());

    let listed_after = world_link::list_for_world_at(&inst, "Alpha", None)
        .await
        .unwrap();
    assert_eq!(listed_after.len(), 1);
    assert_eq!(listed_after[0].filename, "vm.zip");
    assert_eq!(listed_after[0].state, WorldPackState::NotAdded);
}

#[tokio::test]
async fn a_hand_dropped_library_file_is_adopted() {
    let (_td, inst) = make_fixture("inst-hand-dropped", &[]);
    let bytes = datapack_zip(48, "Vein Miner");

    let lib_dir = library_dir_at(&inst);
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(lib_dir.join("manual.zip"), &bytes).unwrap();

    let listed = library::list_at(&inst).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filename, "manual.zip");
    assert_eq!(
        listed[0].source, None,
        "no provenance for a hand-dropped file"
    );
    assert_eq!(listed[0].pack_format, Some(48));
    assert_eq!(listed[0].name, "Vein Miner");
}
