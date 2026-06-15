//! `servers.dat` read / safe-edit on a `fastnbt::Value` model.
//!
//! We operate on the raw `Value` (not a typed struct) so that fields we do
//! not model — `icon`, `hidden`, `acceptTextures`, future keys, and any
//! root-level keys — round-trip untouched. servers.dat is UNCOMPRESSED NBT,
//! so `fastnbt::{from_bytes, to_bytes}` apply directly (no gzip layer).

use crate::error::{Error, Result};
use crate::servers::SavedServer;
use fastnbt::Value;
use std::collections::HashMap;

/// Parse raw `servers.dat` bytes into the root NBT value.
pub fn parse(bytes: &[u8]) -> Result<Value> {
    fastnbt::from_bytes(bytes).map_err(|e| Error::ServersDatParse {
        reason: e.to_string(),
    })
}

/// Serialize the root NBT value back to `servers.dat` bytes.
pub fn serialize(root: &Value) -> Result<Vec<u8>> {
    fastnbt::to_bytes(root).map_err(|e| Error::ServersDatParse {
        reason: e.to_string(),
    })
}

/// A fresh root compound with an empty `servers` list. Used when the file
/// does not exist yet and the user adds the first server.
pub fn empty_root() -> Value {
    let mut map = HashMap::new();
    map.insert("servers".to_string(), Value::List(Vec::new()));
    Value::Compound(map)
}

/// Read-only projection: every entry that has a string `ip`, in file order.
/// Entries missing `ip` are skipped (defensive — a malformed entry MC would
/// ignore too). A missing/!compound root or `servers` key yields an empty Vec.
pub fn list_view(root: &Value) -> Vec<SavedServer> {
    let Value::Compound(map) = root else {
        return Vec::new();
    };
    let Some(Value::List(list)) = map.get("servers") else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|v| {
            let Value::Compound(e) = v else { return None };
            let address = match e.get("ip") {
                Some(Value::String(s)) => s.clone(),
                _ => return None,
            };
            let name = match e.get("name") {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            Some(SavedServer { name, address })
        })
        .collect()
}

/// Borrow the `servers` list mutably, creating an empty one if absent.
/// Errors if the root is not a compound or `servers` is present but not a list.
fn servers_list_mut(root: &mut Value) -> Result<&mut Vec<Value>> {
    let Value::Compound(map) = root else {
        return Err(Error::ServersDatParse {
            reason: "root is not a compound".into(),
        });
    };
    let slot = map
        .entry("servers".to_string())
        .or_insert_with(|| Value::List(Vec::new()));
    match slot {
        Value::List(list) => Ok(list),
        _ => Err(Error::ServersDatParse {
            reason: "'servers' is not a list".into(),
        }),
    }
}

/// Append a new `{ name, ip }` compound to the end of the list.
pub fn push_server(root: &mut Value, name: &str, address: &str) -> Result<()> {
    let list = servers_list_mut(root)?;
    let mut e = HashMap::new();
    e.insert("name".to_string(), Value::String(name.to_string()));
    e.insert("ip".to_string(), Value::String(address.to_string()));
    list.push(Value::Compound(e));
    Ok(())
}

/// Remove the entry at `index`, but ONLY if its `ip` equals `expected_address`
/// (stale-guard against a list that changed since the UI last read it). Any
/// mismatch / out-of-bounds → `SavedServerListChanged`, file left unchanged.
pub fn remove_server(root: &mut Value, index: usize, expected_address: &str) -> Result<()> {
    let list = servers_list_mut(root)?;
    let address_matches = list
        .get(index)
        .and_then(|v| match v {
            Value::Compound(e) => e.get("ip"),
            _ => None,
        })
        .map(|ip| matches!(ip, Value::String(s) if s == expected_address))
        .unwrap_or(false);
    if !address_matches {
        return Err(Error::SavedServerListChanged);
    }
    list.remove(index);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;
    use std::collections::HashMap;

    fn entry(name: &str, ip: &str) -> Value {
        let mut m = HashMap::new();
        m.insert("name".to_string(), Value::String(name.to_string()));
        m.insert("ip".to_string(), Value::String(ip.to_string()));
        Value::Compound(m)
    }

    #[test]
    fn list_view_extracts_name_and_address() {
        let mut root_map = HashMap::new();
        root_map.insert(
            "servers".to_string(),
            Value::List(vec![
                entry("SMP", "play.example.net"),
                entry("Anom", "mc.x:25566"),
            ]),
        );
        let root = Value::Compound(root_map);
        let view = list_view(&root);
        assert_eq!(view.len(), 2);
        assert_eq!(
            view[0],
            SavedServer {
                name: "SMP".into(),
                address: "play.example.net".into()
            }
        );
        assert_eq!(view[1].address, "mc.x:25566");
    }

    #[test]
    fn missing_servers_key_yields_empty_view() {
        assert!(list_view(&Value::Compound(HashMap::new())).is_empty());
    }

    #[test]
    fn round_trip_preserves_unknown_entry_fields() {
        // Build an entry with an extra "hidden" byte + "icon" string that we
        // do NOT model; it must survive a parse -> push -> serialize -> parse.
        let mut e = HashMap::new();
        e.insert("name".to_string(), Value::String("Keep".to_string()));
        e.insert("ip".to_string(), Value::String("keep.example".to_string()));
        e.insert("icon".to_string(), Value::String("BASE64PNG".to_string()));
        e.insert("hidden".to_string(), Value::Byte(1));
        let mut root_map = HashMap::new();
        root_map.insert("servers".to_string(), Value::List(vec![Value::Compound(e)]));
        // An unrelated root-level key must also survive.
        root_map.insert("schemaVersion".to_string(), Value::Int(2));
        let root = Value::Compound(root_map);

        let bytes = serialize(&root).unwrap();
        let mut parsed = parse(&bytes).unwrap();
        push_server(&mut parsed, "New", "new.example").unwrap();
        let bytes2 = serialize(&parsed).unwrap();
        let parsed2 = parse(&bytes2).unwrap();

        // New entry present.
        let view = list_view(&parsed2);
        assert_eq!(view.len(), 2);
        assert_eq!(
            view[1],
            SavedServer {
                name: "New".into(),
                address: "new.example".into()
            }
        );
        // Original extra fields preserved.
        let Value::Compound(root2) = &parsed2 else {
            panic!("root not compound")
        };
        assert!(matches!(root2.get("schemaVersion"), Some(Value::Int(2))));
        let Some(Value::List(list)) = root2.get("servers") else {
            panic!("no servers list")
        };
        let Value::Compound(first) = &list[0] else {
            panic!("entry not compound")
        };
        assert!(matches!(first.get("icon"), Some(Value::String(s)) if s == "BASE64PNG"));
        assert!(matches!(first.get("hidden"), Some(Value::Byte(1))));
    }

    #[test]
    fn remove_server_with_matching_address_succeeds() {
        let mut root_map = HashMap::new();
        root_map.insert(
            "servers".to_string(),
            Value::List(vec![entry("A", "a.example"), entry("B", "b.example")]),
        );
        let mut root = Value::Compound(root_map);
        remove_server(&mut root, 0, "a.example").unwrap();
        let view = list_view(&root);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].address, "b.example");
    }

    #[test]
    fn remove_server_stale_guard_rejects_mismatch() {
        let mut root_map = HashMap::new();
        root_map.insert(
            "servers".to_string(),
            Value::List(vec![entry("A", "a.example")]),
        );
        let mut root = Value::Compound(root_map);
        let err = remove_server(&mut root, 0, "different.example").unwrap_err();
        assert!(matches!(err, crate::error::Error::SavedServerListChanged));
        // Nothing removed.
        assert_eq!(list_view(&root).len(), 1);
    }

    #[test]
    fn remove_server_out_of_bounds_is_stale() {
        let mut root = empty_root();
        let err = remove_server(&mut root, 5, "x").unwrap_err();
        assert!(matches!(err, crate::error::Error::SavedServerListChanged));
    }

    #[test]
    fn empty_root_serializes_and_reparses_to_empty_view() {
        let root = empty_root();
        let bytes = serialize(&root).unwrap();
        assert!(list_view(&parse(&bytes).unwrap()).is_empty());
    }
}
