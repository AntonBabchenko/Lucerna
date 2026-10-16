//! Per kind, what the instance's file held when Lucerna last touched it (the
//! baseline), and transiently what Lucerna is about to write (pending).
//!
//! Stored as NBT, not JSON: a projection is NBT values, and JSON would lose
//! their types (a byte flag would come back an int and compare unequal).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fastnbt::Value;

use super::{state_dir, Kind};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    Baseline,
    Pending,
}

/// A projection with the versions it was taken under.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snapshot {
    /// The instance's Minecraft version.
    pub mc: String,
    /// The file's own version stamp (hotbars' `DataVersion`), when it has one.
    pub stamp: Option<i32>,
    pub data: Value,
}

pub(crate) fn path(instance_root: &Path, kind: Kind, slot: Slot) -> PathBuf {
    let ext = match slot {
        Slot::Baseline => "baseline",
        Slot::Pending => "pending",
    };
    state_dir(instance_root).join(format!("{}.{ext}", kind.stem()))
}

pub(crate) fn read(instance_root: &Path, kind: Kind, slot: Slot) -> Result<Option<Snapshot>> {
    let p = path(instance_root, kind, slot);
    let Some(bytes) = super::fsio::read_if_present(&p)? else {
        return Ok(None);
    };
    let bad = |why: &str| Error::io(p.display().to_string(), format!("parse: {why}"));
    let root: Value = fastnbt::from_bytes(&bytes).map_err(|e| bad(&e.to_string()))?;
    let Value::Compound(mut map) = root else {
        return Err(bad("the root is not a compound"));
    };
    let mc = match map.remove("mc") {
        Some(Value::String(mc)) => mc,
        _ => return Err(bad("no Minecraft version")),
    };
    let stamp = match map.remove("stamp") {
        Some(Value::Int(n)) => Some(n),
        None => None,
        Some(_) => return Err(bad("the stamp is not an int")),
    };
    let data = map.remove("data").ok_or_else(|| bad("no data"))?;
    Ok(Some(Snapshot { mc, stamp, data }))
}

pub(crate) fn write(instance_root: &Path, kind: Kind, slot: Slot, snap: &Snapshot) -> Result<()> {
    let p = path(instance_root, kind, slot);
    let mut map = HashMap::new();
    map.insert("mc".to_string(), Value::String(snap.mc.clone()));
    if let Some(n) = snap.stamp {
        map.insert("stamp".to_string(), Value::Int(n));
    }
    map.insert("data".to_string(), snap.data.clone());
    let bytes = fastnbt::to_bytes(&Value::Compound(map))
        .map_err(|e| Error::io(p.display().to_string(), format!("serialize: {e}")))?;
    super::fsio::write_atomic(&p, &bytes)
}

pub(crate) fn remove(instance_root: &Path, kind: Kind, slot: Slot) -> Result<()> {
    super::fsio::remove_if_present(&path(instance_root, kind, slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        let mut entry = HashMap::new();
        entry.insert("ip".to_string(), Value::String("mc.example.net".into()));
        entry.insert("hidden".to_string(), Value::Byte(1));
        Snapshot {
            mc: "1.21.1".into(),
            stamp: Some(3955),
            data: Value::List(vec![Value::Compound(entry)]),
        }
    }

    #[test]
    fn a_snapshot_round_trips_with_its_types() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), Kind::Servers, Slot::Baseline, &snap()).unwrap();
        assert_eq!(
            read(td.path(), Kind::Servers, Slot::Baseline).unwrap(),
            Some(snap())
        );
    }

    #[test]
    fn a_snapshot_without_a_stamp_reads_back_without_one() {
        let td = tempfile::tempdir().unwrap();
        let s = Snapshot {
            stamp: None,
            ..snap()
        };
        write(td.path(), Kind::History, Slot::Pending, &s).unwrap();
        assert_eq!(
            read(td.path(), Kind::History, Slot::Pending).unwrap(),
            Some(s)
        );
    }

    #[test]
    fn absent_is_none_and_garbage_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            read(td.path(), Kind::Hotbars, Slot::Baseline).unwrap(),
            None
        );
        let p = path(td.path(), Kind::Hotbars, Slot::Baseline);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"not nbt").unwrap();
        assert!(read(td.path(), Kind::Hotbars, Slot::Baseline).is_err());
    }
}
