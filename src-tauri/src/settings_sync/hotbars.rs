//! hotbar.nbt (spec §B6): nine saved rows (`"0"`–`"8"`) and the
//! `DataVersion` that wrote them. A row changed here replaces that row in the
//! shared file; the shared `DataVersion` is the highest that ever wrote to it,
//! and an older game neither receives nor captures (the engine's gate).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use fastnbt::Value;

use super::failure::{self, At};
use super::Kind;
use crate::datapacks::level_dat::{self, Framing};
use crate::error::Result;

const DATA_VERSION: &str = "DataVersion";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hotbars {
    pub rows: BTreeMap<String, Value>,
    pub data_version: Option<i32>,
    pub framing: Framing,
}

/// The instance's hotbars; `None` when there is no file.
pub(crate) fn read(path: &Path) -> Result<Option<Hotbars>> {
    let at = At::game(Kind::Hotbars);
    let Some(bytes) = super::fsio::read_if_present(at, path)? else {
        return Ok(None);
    };
    let (root, framing) = level_dat::parse(&bytes).map_err(|e| failure::damaged(at, path, e))?;
    from_root(root, framing)
        .map(Some)
        .map_err(|why| failure::damaged(at, path, why))
}

pub(crate) fn from_root(root: Value, framing: Framing) -> std::result::Result<Hotbars, String> {
    let Value::Compound(mut map) = root else {
        return Err("the root is not a compound".into());
    };
    let data_version = match map.remove(DATA_VERSION) {
        Some(Value::Int(n)) => Some(n),
        None => None,
        Some(_) => return Err("DataVersion is not an int".into()),
    };
    Ok(Hotbars {
        rows: map.into_iter().collect(),
        data_version,
        framing,
    })
}

/// The rows as one compound (a baseline's data).
pub(crate) fn rows_value(h: &Hotbars) -> Value {
    Value::Compound(h.rows.clone().into_iter().collect::<HashMap<_, _>>())
}

pub(crate) fn to_bytes(h: &Hotbars) -> Result<Vec<u8>> {
    let mut map: HashMap<String, Value> = h.rows.clone().into_iter().collect();
    if let Some(n) = h.data_version {
        map.insert(DATA_VERSION.to_string(), Value::Int(n));
    }
    level_dat::serialize(&Value::Compound(map), h.framing)
}

/// Merge the rows changed here since `baseline` into `shared`; the shared
/// `DataVersion` becomes the higher of its own and the writer's.
pub(crate) fn capture(
    baseline: &Hotbars,
    now: &Hotbars,
    shared: &Hotbars,
    writer_dv: i32,
) -> Hotbars {
    let mut out = shared.clone();
    for (key, row) in &now.rows {
        if baseline.rows.get(key) != Some(row) {
            out.rows.insert(key.clone(), row.clone());
        }
    }
    out.data_version = Some(shared.data_version.map_or(writer_dv, |n| n.max(writer_dv)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(item: &str) -> Value {
        let mut slot = HashMap::new();
        slot.insert("id".to_string(), Value::String(item.into()));
        slot.insert("count".to_string(), Value::Int(1));
        Value::List(vec![Value::Compound(slot)])
    }

    fn bars(rows: &[(&str, &str)], dv: i32) -> Hotbars {
        Hotbars {
            rows: rows.iter().map(|(k, v)| (k.to_string(), row(v))).collect(),
            data_version: Some(dv),
            framing: Framing::Raw,
        }
    }

    #[test]
    fn a_row_changed_here_replaces_that_row_only() {
        let base = bars(&[("0", "stone"), ("1", "dirt")], 3955);
        let now = bars(&[("0", "diamond"), ("1", "dirt")], 3955);
        let shared = bars(&[("0", "stone"), ("1", "glass")], 3955);
        let got = capture(&base, &now, &shared, 3955);
        assert_eq!(got.rows["0"], row("diamond"));
        assert_eq!(
            got.rows["1"],
            row("glass"),
            "another instance's row survives"
        );
    }

    #[test]
    fn a_row_missing_here_is_not_a_deletion() {
        let base = bars(&[("0", "stone"), ("1", "dirt")], 3955);
        let now = bars(&[("0", "stone")], 3955);
        let shared = base.clone();
        assert_eq!(capture(&base, &now, &shared, 3955).rows["1"], row("dirt"));
    }

    #[test]
    fn the_shared_data_version_only_goes_up() {
        let base = bars(&[("0", "stone")], 3955);
        let now = bars(&[("0", "glass")], 4300);
        let shared = bars(&[("0", "stone")], 3955);
        assert_eq!(capture(&base, &now, &shared, 4300).data_version, Some(4300));
        assert_eq!(
            capture(&base, &now, &bars(&[], 4400), 4300).data_version,
            Some(4400)
        );
    }

    #[test]
    fn the_file_round_trips() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("hotbar.nbt");
        assert_eq!(read(&p).unwrap(), None);
        let h = bars(&[("0", "stone"), ("8", "torch")], 3955);
        std::fs::write(&p, to_bytes(&h).unwrap()).unwrap();
        assert_eq!(read(&p).unwrap(), Some(h));
    }
}
