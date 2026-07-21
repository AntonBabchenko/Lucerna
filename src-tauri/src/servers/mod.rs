//! Per-instance saved multiplayer servers, backed by the instance's own
//! `<instance>/.minecraft/servers.dat` (uncompressed NBT). Local-file-only;
//! this module makes no network calls. The connect action itself reuses the
//! existing Quick Play launch path — this module only reads/edits the list.

pub mod nbt;

use serde::Serialize;
use specta::Type;

/// One saved server, surfaced to the UI. `address` mirrors the NBT `ip` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SavedServer {
    pub name: String,
    pub address: String,
}

use crate::error::{Error, Result};
use std::path::PathBuf;

const MAX_SERVER_NAME_LEN: usize = 128;

/// `<instance>/.minecraft/servers.dat`.
fn servers_dat_path(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf> {
    crate::paths::minecraft_dir(app, instance_id)
        .map(|p| p.join("servers.dat"))
        .map_err(|e| Error::io("<servers.dat>", e))
}

/// Validate a display name at the IPC boundary: non-empty (trimmed), no
/// control characters, at most `MAX_SERVER_NAME_LEN` unicode scalars.
fn validate_server_name(name: &str) -> Result<()> {
    let invalid = |reason: &str| Error::SavedServerNameInvalid {
        name: name.to_string(),
        reason: reason.to_string(),
    };
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(invalid("empty name"));
    }
    if trimmed.chars().count() > MAX_SERVER_NAME_LEN {
        return Err(invalid("name too long"));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(invalid("contains control characters"));
    }
    Ok(())
}

/// Atomic write: write to a sibling temp file, then rename over the target.
/// Creates the parent `.minecraft` dir if missing.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    let renamed = std::fs::rename(&tmp, path).map_err(|e| Error::io(path.display().to_string(), e));
    if renamed.is_err() {
        // Don't leave a stale servers.tmp behind on a failed rename.
        let _ = std::fs::remove_file(&tmp);
    }
    renamed
}

/// List the instance's saved servers. Missing file → empty Vec (not an error).
/// A corrupt file surfaces as `ServersDatParse` (the file is NOT overwritten).
pub fn list_saved_servers(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<SavedServer>> {
    let path = servers_dat_path(app, instance_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(nbt::list_view(&nbt::parse(&bytes)?))
}

/// Append a server. Blocked while Minecraft is running (it rewrites
/// servers.dat on exit and would clobber the launcher's write).
pub fn add_saved_server(
    app: &tauri::AppHandle,
    instance_id: &str,
    name: &str,
    address: &str,
) -> Result<()> {
    // servers.dat is per-instance: block only while THIS instance is running,
    // because its Minecraft rewrites this file on exit and would clobber our write.
    if crate::launch::is_running(instance_id) {
        return Err(Error::InstanceBusy);
    }
    validate_server_name(name)?;
    crate::launch::quick_play::validate_server_address(address)?;
    let path = servers_dat_path(app, instance_id)?;
    let mut root = if path.exists() {
        let bytes = std::fs::read(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
        nbt::parse(&bytes)?
    } else {
        nbt::empty_root()
    };
    nbt::push_server(&mut root, name.trim(), address)?;
    write_atomic(&path, &nbt::serialize(&root)?)
}

/// Remove the server at `index`, guarded by `expected_address`. Blocked while
/// Minecraft is running. Missing file or a changed list → `SavedServerListChanged`.
pub fn remove_saved_server(
    app: &tauri::AppHandle,
    instance_id: &str,
    index: usize,
    expected_address: &str,
) -> Result<()> {
    // servers.dat is per-instance: block only while THIS instance is running,
    // because its Minecraft rewrites this file on exit and would clobber our write.
    if crate::launch::is_running(instance_id) {
        return Err(Error::InstanceBusy);
    }
    let path = servers_dat_path(app, instance_id)?;
    if !path.exists() {
        return Err(Error::SavedServerListChanged);
    }
    let bytes = std::fs::read(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let mut root = nbt::parse(&bytes)?;
    nbt::remove_server(&mut root, index, expected_address)?;
    write_atomic(&path, &nbt::serialize(&root)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn validate_name_accepts_normal() {
        assert!(validate_server_name("My SMP").is_ok());
        assert!(validate_server_name("Сервер друзей").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty_and_whitespace() {
        assert!(matches!(
            validate_server_name("   "),
            Err(Error::SavedServerNameInvalid { .. })
        ));
    }

    #[test]
    fn validate_name_rejects_control_chars() {
        assert!(matches!(
            validate_server_name("bad\u{0}name"),
            Err(Error::SavedServerNameInvalid { .. })
        ));
    }

    #[test]
    fn validate_name_rejects_overlong() {
        let long = "a".repeat(MAX_SERVER_NAME_LEN + 1);
        assert!(matches!(
            validate_server_name(&long),
            Err(Error::SavedServerNameInvalid { .. })
        ));
    }
}
