//! A world's `level.dat`: gzip-or-raw NBT, edited as an untyped
//! `fastnbt::Value` so every tag we do not model — `Player`, `GameRules`,
//! `WorldGenSettings`, and ~40 others — round-trips untouched. A typed struct
//! would destroy them on save. Same discipline as `crate::servers::nbt`, which
//! this is modelled on; the difference is the compression layer.
//!
//! Two rules that are easy to get wrong:
//!   * Never compare `level.dat` byte-for-byte. `Value::Compound` is a
//!     `HashMap`, so key order is lost on rewrite. Compare parsed `Value`s.
//!   * That every real `level.dat` is gzip-framed is a Minecraft-format
//!     assumption, not something this repo can prove. Sniff the magic and
//!     re-emit in whatever framing was read.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use fastnbt::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Gzip,
    Raw,
}

/// A level.dat is a few KB. The cap is not about the honest case — it is
/// about a crafted level.dat (from an imported world or a modpack override,
/// both already treated as untrusted) inflating a gzip stream (up to
/// ~1032:1) into a `Value` tree that costs far more per byte than the wire
/// encoding — a `TAG_List` of `TAG_Byte` costs 1 byte on disk and roughly 56
/// bytes as a `Value`. Left unbounded, that is not a caught error, it is an
/// allocation failure, and Rust **aborts** the process.
const MAX_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;

fn parse_err(e: impl std::fmt::Display) -> Error {
    Error::LevelDatParse {
        reason: e.to_string(),
    }
}

/// Parse `bytes`, detecting the framing from the gzip magic `1f 8b`.
pub fn parse(bytes: &[u8]) -> Result<(Value, Framing)> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_DECOMPRESSED_BYTES)
            .read_to_end(&mut buf)
            .map_err(|e| parse_err(format!("gzip: {e}")))?;
        // `.take` silently stops at the cap instead of erroring, so a stream
        // that was truncated here would otherwise reach `from_bytes` and
        // fail with a confusing NBT error instead of an honest one about size.
        if buf.len() as u64 == MAX_DECOMPRESSED_BYTES {
            return Err(parse_err(format!(
                "gzip stream exceeds the {MAX_DECOMPRESSED_BYTES}-byte decompressed cap"
            )));
        }
        let v = fastnbt::from_bytes(&buf).map_err(parse_err)?;
        Ok((v, Framing::Gzip))
    } else {
        let v = fastnbt::from_bytes(bytes).map_err(parse_err)?;
        Ok((v, Framing::Raw))
    }
}

/// Serialize in the given framing.
pub fn serialize(root: &Value, framing: Framing) -> Result<Vec<u8>> {
    let plain = fastnbt::to_bytes(root).map_err(parse_err)?;
    match framing {
        Framing::Raw => Ok(plain),
        Framing::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&plain)
                .map_err(|e| parse_err(format!("gzip: {e}")))?;
            enc.finish().map_err(|e| parse_err(format!("gzip: {e}")))
        }
    }
}

/// Read the two lists. Absent or malformed shapes read as empty rather than
/// erroring — listing a world must never fail because its level.dat is odd.
pub fn lists(root: &Value) -> (Vec<String>, Vec<String>) {
    let Value::Compound(map) = root else {
        return (Vec::new(), Vec::new());
    };
    let Some(Value::Compound(data)) = map.get("Data") else {
        return (Vec::new(), Vec::new());
    };
    let Some(Value::Compound(dp)) = data.get("DataPacks") else {
        return (Vec::new(), Vec::new());
    };
    (
        string_list(dp.get("Enabled")),
        string_list(dp.get("Disabled")),
    )
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    let Some(Value::List(items)) = v else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| match i {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Mutable access to `Data.DataPacks`, creating the compounds when absent and
/// rejecting a key that exists with the wrong tag type. Mirrors
/// `servers::nbt::servers_list_mut`.
fn datapacks_mut(root: &mut Value) -> Result<&mut HashMap<String, Value>> {
    let Value::Compound(map) = root else {
        return Err(parse_err("level.dat root is not a compound"));
    };
    let data_slot = map
        .entry("Data".to_string())
        .or_insert_with(|| Value::Compound(HashMap::new()));
    let Value::Compound(data) = data_slot else {
        return Err(parse_err("level.dat Data is not a compound"));
    };
    let dp_slot = data
        .entry("DataPacks".to_string())
        .or_insert_with(|| Value::Compound(HashMap::new()));
    match dp_slot {
        Value::Compound(dp) => Ok(dp),
        _ => Err(parse_err("level.dat Data.DataPacks is not a compound")),
    }
}

fn list_mut<'a>(dp: &'a mut HashMap<String, Value>, key: &str) -> Result<&'a mut Vec<Value>> {
    let slot = dp
        .entry(key.to_string())
        .or_insert_with(|| Value::List(Vec::new()));
    match slot {
        // An empty list passes `.all()` vacuously — the common case (a world
        // with no packs recorded yet), and correct: there is nothing for it
        // to be heterogeneous with.
        Value::List(list) if list.iter().all(|v| matches!(v, Value::String(_))) => Ok(list),
        // A non-string element is already present. Appending a `Value::String`
        // to it would build a list fastnbt CAN write but Minecraft cannot
        // read: the wire format's element-type byte comes from the first
        // element only, so a mixed list serializes as one tag type with the
        // wrong-shaped payloads behind it, desynchronising every tag that
        // follows in the file.
        Value::List(_) => Err(parse_err(format!(
            "level.dat Data.DataPacks.{key} is not a list of strings"
        ))),
        _ => Err(parse_err(format!(
            "level.dat Data.DataPacks.{key} is not a list"
        ))),
    }
}

fn drop_entry(list: &mut Vec<Value>, entry: &str) {
    list.retain(|v| !matches!(v, Value::String(s) if s == entry));
}

fn push_unique(list: &mut Vec<Value>, entry: &str) {
    if !list
        .iter()
        .any(|v| matches!(v, Value::String(s) if s == entry))
    {
        list.push(Value::String(entry.to_string()));
    }
}

/// Put `entry` (a `file/<filename>` value) in exactly one of the two lists.
/// Idempotent.
pub fn set_enabled(root: &mut Value, entry: &str, enabled: bool) -> Result<()> {
    let dp = datapacks_mut(root)?;
    // Take both lists in turn — the borrow checker will not hand out two
    // mutable borrows of the same map at once.
    {
        let from = if enabled { "Disabled" } else { "Enabled" };
        drop_entry(list_mut(dp, from)?, entry);
    }
    {
        let to = if enabled { "Enabled" } else { "Disabled" };
        push_unique(list_mut(dp, to)?, entry);
    }
    Ok(())
}

/// Remove `entry` from both lists. Idempotent — this is what clears the
/// "data packs are no longer present" screen after a pack is removed.
pub fn forget(root: &mut Value, entry: &str) -> Result<()> {
    let dp = datapacks_mut(root)?;
    drop_entry(list_mut(dp, "Enabled")?, entry);
    drop_entry(list_mut(dp, "Disabled")?, entry);
    Ok(())
}

/// The world folder's own name, for user-facing messages. `world_dir` is
/// always the caller-supplied world directory itself (never derived from
/// some other path), so this can only come back empty for a path with no
/// normal final component — not a real world directory.
fn folder_name_of(world_dir: &Path) -> String {
    world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Maps a `std::fs::read` failure on `level.dat` to a typed error. A locked
/// level.dat means "quit Minecraft", not "your world is corrupt" — telling a
/// user their world is unparseable invites destructive recovery
/// (delete-and-recreate, restore a stale backup) for a problem that goes away
/// on its own once the game closes.
///
/// Windows-only: access denied (5), sharing violation (32) and lock
/// violation (33) are the codes a held-open file surfaces there. On POSIX,
/// `rename(2)` over a file another process has open SUCCEEDS, so a write
/// against a world Minecraft has open on Linux/macOS is not caught by this
/// mapping — or by this module — at all; Minecraft's next autosave silently
/// overwrites what we wrote. A caller that must not race a running world has
/// to consult the running-instance registry itself.
fn map_read_err(path: &Path, e: std::io::Error, world_dir: &Path) -> Error {
    if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
        Error::WorldInUse {
            folder_name: folder_name_of(world_dir),
        }
    } else {
        Error::io(path.display().to_string(), e)
    }
}

/// Read `<world_dir>/level.dat`.
pub fn read_at(world_dir: &Path) -> Result<(Value, Framing)> {
    let path = world_dir.join("level.dat");
    let bytes = std::fs::read(&path).map_err(|e| map_read_err(&path, e, world_dir))?;
    parse(&bytes)
}

/// Rewrite `<world_dir>/level.dat`, keeping the pre-edit bytes in
/// `level.dat_lucerna.bak`.
///
/// Deliberately NOT `level.dat_old` — that is Minecraft's own recovery copy
/// and overwriting it would trade away the user's fallback.
///
/// Both writes go through `store::place_bytes` (temp + rename), so a
/// *process* crash can never leave a half-written level.dat. `place_bytes`
/// does not fsync, so a power-loss during the rename window can still lose
/// the tail of whichever file was mid-write — Minecraft's own `level.dat_old`
/// remains the last line of defence against that case.
pub async fn write_at(world_dir: &Path, root: &Value, framing: Framing) -> Result<()> {
    let path = world_dir.join("level.dat");
    let new_bytes = serialize(root, framing)?;

    // Verify what we are about to write can be read back, before touching
    // disk at all: a level.dat is a few KB, so the reparse is free next to
    // the consequence of silently writing something unopenable. Deliberately
    // NOT whole-Value equality — `Value::Float`/`Value::Double` derive
    // `PartialEq`, so a world whose `Data.Player.Pos` holds a NaN (a real
    // corruption state this function might be asked to repair) would fail
    // equality and block a legitimate write. Checking that the
    // enabled/disabled lists read back as intended catches structural
    // corruption and truncation without that false positive.
    let (reparsed, _) = parse(&new_bytes)?;
    if lists(&reparsed) != lists(root) {
        return Err(parse_err(
            "level.dat re-read did not match the edit; not writing",
        ));
    }

    // Never overwrite level.dat unless the pre-edit bytes are either safely
    // copied to the backup, or provably absent. `if let Ok(old) = read(..)`
    // would treat every non-`NotFound` read failure — permission denied, a
    // lock, a bad sector — the same as "no file yet" and overwrite anyway: on
    // POSIX, `rename(2)` needs write+execute on the *directory* and no
    // permission on the destination file itself, so a level.dat we cannot
    // read can still rename over fine. That would silently destroy the only
    // copy of the world's metadata.
    match std::fs::read(&path) {
        Ok(old) => {
            let bak = world_dir.join("level.dat_lucerna.bak");
            crate::mods::store::place_bytes(&bak, &old)
                .await
                .map_err(|e| map_store_err(&e))?;
            // The entire safety story here rests on a file the user has
            // never heard of — name it so a shared log points at the
            // recovery copy.
            crate::diag!("datapacks: backed up {} before rewriting", bak.display());
        }
        // Absent is the only benign case: a world with no level.dat yet.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // The file exists and we could not read it to back it up. Never
        // overwrite what we could not preserve.
        Err(e) => return Err(map_read_err(&path, e, world_dir)),
    }

    crate::mods::store::place_bytes(&path, &new_bytes)
        .await
        .map_err(|e| map_store_err(&e))
}

/// A running Minecraft holds level.dat open — surface that as the friendly
/// typed `WorldInUse` rather than a raw IO string. See `map_read_err` for the
/// identical mapping applied to a plain read, and its POSIX caveat, which
/// applies here unchanged.
fn map_store_err(e: &crate::mods::store::StoreIoError) -> Error {
    if matches!(e.source.raw_os_error(), Some(5) | Some(32) | Some(33)) {
        let folder_name = e
            .path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Error::WorldInUse { folder_name }
    } else {
        Error::io(e.path.display().to_string(), e.details())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;
    use std::collections::HashMap;

    fn sample_root() -> Value {
        let mut data = HashMap::new();
        data.insert("LevelName".to_string(), Value::String("Survival".into()));
        data.insert("SpawnX".to_string(), Value::Int(42));
        let mut root = HashMap::new();
        root.insert("Data".to_string(), Value::Compound(data));
        Value::Compound(root)
    }

    #[test]
    fn gzip_round_trip_reparses_to_the_same_value() {
        let bytes = serialize(&sample_root(), Framing::Gzip).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b], "must be gzip framed");
        let (back, framing) = parse(&bytes).unwrap();
        assert_eq!(framing, Framing::Gzip);
        // Compare parsed Values, never bytes: fastnbt's Compound is a HashMap,
        // so key order is not preserved across a rewrite.
        assert_eq!(back, sample_root());
    }

    #[test]
    fn raw_round_trip_reparses_to_the_same_value() {
        let bytes = serialize(&sample_root(), Framing::Raw).unwrap();
        assert_ne!(&bytes[..2], &[0x1f, 0x8b]);
        let (back, framing) = parse(&bytes).unwrap();
        assert_eq!(framing, Framing::Raw);
        assert_eq!(back, sample_root());
    }

    #[test]
    fn parse_detects_framing_from_the_magic_not_from_a_flag() {
        let raw = serialize(&sample_root(), Framing::Raw).unwrap();
        let gz = serialize(&sample_root(), Framing::Gzip).unwrap();
        assert_eq!(parse(&raw).unwrap().1, Framing::Raw);
        assert_eq!(parse(&gz).unwrap().1, Framing::Gzip);
    }

    #[test]
    fn garbage_yields_level_dat_parse_not_a_panic() {
        let err = parse(b"absolutely not nbt").unwrap_err();
        assert!(matches!(err, crate::error::Error::LevelDatParse { .. }));
    }

    fn root_with_unknown_keys() -> Value {
        let mut data = HashMap::new();
        data.insert("LevelName".to_string(), Value::String("Survival".into()));
        data.insert("RandomSeed".to_string(), Value::Long(12345));
        let mut gamerules = HashMap::new();
        gamerules.insert("keepInventory".to_string(), Value::String("true".into()));
        data.insert("GameRules".to_string(), Value::Compound(gamerules));
        let mut root = HashMap::new();
        root.insert("Data".to_string(), Value::Compound(data));
        root.insert("SomethingWeNeverModelled".to_string(), Value::Byte(7));
        Value::Compound(root)
    }

    #[test]
    fn lists_are_empty_when_datapacks_is_absent() {
        let root = root_with_unknown_keys();
        let (enabled, disabled) = lists(&root);
        assert!(enabled.is_empty());
        assert!(disabled.is_empty());
    }

    #[test]
    fn set_enabled_true_adds_to_enabled_and_removes_from_disabled() {
        let mut root = root_with_unknown_keys();
        set_enabled(&mut root, "file/vm.zip", false).unwrap();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        let (enabled, disabled) = lists(&root);
        assert_eq!(enabled, vec!["file/vm.zip".to_string()]);
        assert!(disabled.is_empty());
    }

    #[test]
    fn set_enabled_false_moves_the_entry_to_disabled() {
        let mut root = root_with_unknown_keys();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        set_enabled(&mut root, "file/vm.zip", false).unwrap();
        let (enabled, disabled) = lists(&root);
        assert!(enabled.is_empty());
        assert_eq!(disabled, vec!["file/vm.zip".to_string()]);
    }

    #[test]
    fn set_enabled_is_idempotent() {
        let mut root = root_with_unknown_keys();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        assert_eq!(lists(&root).0, vec!["file/vm.zip".to_string()]);
    }

    #[test]
    fn forget_removes_the_entry_from_both_lists() {
        let mut root = root_with_unknown_keys();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        set_enabled(&mut root, "file/other.zip", false).unwrap();
        forget(&mut root, "file/vm.zip").unwrap();
        forget(&mut root, "file/other.zip").unwrap();
        let (enabled, disabled) = lists(&root);
        assert!(enabled.is_empty());
        assert!(disabled.is_empty());
    }

    #[test]
    fn an_edit_preserves_every_unmodelled_key_through_a_gzip_round_trip() {
        let mut root = root_with_unknown_keys();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        let bytes = serialize(&root, Framing::Gzip).unwrap();
        let (back, _) = parse(&bytes).unwrap();

        let Value::Compound(map) = &back else {
            panic!("root must be a compound")
        };
        assert_eq!(map.get("SomethingWeNeverModelled"), Some(&Value::Byte(7)));
        let Some(Value::Compound(data)) = map.get("Data") else {
            panic!("Data must be a compound")
        };
        assert_eq!(data.get("RandomSeed"), Some(&Value::Long(12345)));
        assert!(matches!(data.get("GameRules"), Some(Value::Compound(_))));
        assert_eq!(
            data.get("LevelName"),
            Some(&Value::String("Survival".into()))
        );
    }

    #[test]
    fn a_wrong_tag_type_is_rejected_rather_than_overwritten() {
        let mut data = HashMap::new();
        data.insert(
            "DataPacks".to_string(),
            Value::String("not a compound".into()),
        );
        let mut root = HashMap::new();
        root.insert("Data".to_string(), Value::Compound(data));
        let mut root = Value::Compound(root);

        let err = set_enabled(&mut root, "file/vm.zip", true).unwrap_err();
        assert!(matches!(err, crate::error::Error::LevelDatParse { .. }));
    }

    #[test]
    fn a_non_compound_root_is_rejected() {
        let mut root = Value::Int(1);
        let err = set_enabled(&mut root, "file/vm.zip", true).unwrap_err();
        assert!(matches!(err, crate::error::Error::LevelDatParse { .. }));
    }

    #[test]
    fn a_heterogeneous_list_is_rejected_rather_than_appended_to() {
        // Enabled already holds a non-string element. Appending a string to
        // it would build a list fastnbt can write but Minecraft cannot read
        // (the wire element-type byte comes from the first element only).
        let mut dp = HashMap::new();
        dp.insert("Enabled".to_string(), Value::List(vec![Value::Int(1)]));
        let mut data = HashMap::new();
        data.insert("DataPacks".to_string(), Value::Compound(dp));
        let mut root_map = HashMap::new();
        root_map.insert("Data".to_string(), Value::Compound(data));
        let mut root = Value::Compound(root_map);

        let err = set_enabled(&mut root, "file/vm.zip", true).unwrap_err();
        assert!(matches!(err, crate::error::Error::LevelDatParse { .. }));
    }

    #[tokio::test]
    async fn write_at_creates_a_backup_and_keeps_the_original_readable() {
        let td = tempfile::tempdir().unwrap();
        let world = td.path();
        let original = serialize(&root_with_unknown_keys(), Framing::Gzip).unwrap();
        std::fs::write(world.join("level.dat"), &original).unwrap();

        let (mut root, framing) = read_at(world).unwrap();
        set_enabled(&mut root, "file/vm.zip", true).unwrap();
        write_at(world, &root, framing).await.unwrap();

        // Backup holds the pre-edit bytes; live file holds the edit.
        assert_eq!(
            std::fs::read(world.join("level.dat_lucerna.bak")).unwrap(),
            original
        );
        let (after, _) = read_at(world).unwrap();
        assert_eq!(lists(&after).0, vec!["file/vm.zip".to_string()]);
    }

    #[tokio::test]
    async fn write_at_without_an_existing_file_writes_no_backup() {
        let td = tempfile::tempdir().unwrap();
        let world = td.path();
        write_at(world, &sample_root(), Framing::Gzip)
            .await
            .unwrap();
        assert!(!world.join("level.dat_lucerna.bak").exists());
        assert!(world.join("level.dat").exists());
    }

    #[tokio::test]
    async fn write_at_refuses_to_overwrite_when_it_cannot_back_up() {
        let td = tempfile::tempdir().unwrap();
        let world = td.path();
        // A directory at level.dat's path makes `fs::read` fail with
        // something other than NotFound on every platform — the same shape
        // as a locked-down file we have no permission to read.
        std::fs::create_dir(world.join("level.dat")).unwrap();

        let result = write_at(world, &sample_root(), Framing::Gzip).await;
        assert!(result.is_err());
        // Nothing was destroyed and nothing was fabricated: the path is
        // still exactly what it was, and no backup was ever created.
        assert!(world.join("level.dat").is_dir());
        assert!(!world.join("level.dat_lucerna.bak").exists());
    }

    #[test]
    fn read_at_reports_a_missing_file_as_an_io_error_not_corruption() {
        let td = tempfile::tempdir().unwrap();
        let err = read_at(td.path()).unwrap_err();
        // Missing is not corrupt: LevelDatParse would tell a user their
        // world is broken when there is simply no world there yet.
        assert!(matches!(err, crate::error::Error::Io { .. }));
    }
}
