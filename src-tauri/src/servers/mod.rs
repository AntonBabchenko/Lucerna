//! Per-instance saved multiplayer servers, backed by the instance's own
//! `<instance>/.minecraft/servers.dat` (uncompressed NBT). The list itself is
//! local-file-only. The connect action reuses the existing Quick Play launch
//! path — this module only reads/edits the list.
//!
//! The one exception to "local files only" is [`ping`]: an opt-in Server List
//! Ping to the user's own saved servers, gated by `network::consent`.

pub mod nbt;
pub mod ping;

use serde::Serialize;
use specta::Type;

/// One saved server, surfaced to the UI. `address` mirrors the NBT `ip` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SavedServer {
    pub name: String,
    pub address: String,
}

use crate::error::{Error, Result};
use crate::settings_sync::{fsio, Kind};
use std::path::{Path, PathBuf};

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

/// List the instance's saved servers. Missing file → empty Vec (not an error).
/// A corrupt file surfaces as `ServersDatParse` (the file is NOT overwritten).
pub fn list_saved_servers(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<SavedServer>> {
    list_at(&servers_dat_path(app, instance_id)?)
}

/// Append a server. Blocked while Minecraft is running (it rewrites
/// servers.dat on exit and would clobber the launcher's write).
pub fn add_saved_server(
    app: &tauri::AppHandle,
    instance_id: &str,
    name: &str,
    address: &str,
) -> Result<()> {
    add_server_at(
        instance_id,
        &servers_dat_path(app, instance_id)?,
        name,
        address,
        || sync_after_edit(app, instance_id),
    )
}

/// Remove the server at `index`, guarded by `expected_address`. Blocked while
/// Minecraft is running. Missing file or a changed list → `SavedServerListChanged`.
pub fn remove_saved_server(
    app: &tauri::AppHandle,
    instance_id: &str,
    index: usize,
    expected_address: &str,
) -> Result<()> {
    remove_server_at(
        instance_id,
        &servers_dat_path(app, instance_id)?,
        index,
        expected_address,
        || sync_after_edit(app, instance_id),
    )
}

/// After an edit, the server-list sync point (spec §B4), so the instances
/// sharing the list see it now rather than at this one's next launch. The
/// edit stands either way: a failure is kept in the sidecar for the Shared
/// data section and logged, and the next sync point still captures the edit
/// (the baseline is only moved by a sync that succeeds).
fn sync_after_edit(app: &tauri::AppHandle, instance_id: &str) {
    if let Err(e) = try_sync_after_edit(app, instance_id) {
        crate::diag!(
            "settings sync: the server list of {instance_id} did not sync after an edit ({e})"
        );
    }
}

fn try_sync_after_edit(app: &tauri::AppHandle, instance_id: &str) -> Result<()> {
    let data_root = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let root =
        crate::paths::instance_dir(app, instance_id).map_err(|e| Error::io("<instance_dir>", e))?;
    let versions = crate::paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let json = crate::paths::instance_json(app, instance_id)
        .map_err(|e| Error::io("<instance_json>", e))?;
    let mc = crate::instances::store::read_instance_json(&json)?.mc_version;
    crate::settings_sync::after_local_edit(&data_root, &root, Kind::Servers, || {
        crate::settings_sync::version::VersionFacts::read(&versions, &mc)
    })
    .map_err(|f| Error::SettingsSyncFailed {
        sync_kind: f.kind,
        file: f.file,
        reason: f.reason,
    })
}

/// The list in `path`. Only a file that does not exist is an empty list; one
/// that cannot be read is an error, never "no servers".
fn list_at(path: &Path) -> Result<Vec<SavedServer>> {
    match fsio::read_if_present(path)? {
        Some(bytes) => Ok(nbt::list_view(&nbt::parse(&bytes)?)),
        None => Ok(Vec::new()),
    }
}

/// [`add_saved_server`] on a path; `then` (the sync point) runs after the
/// write, still under the claim.
///
/// The edit takes the instance's write claim, not just `write_allowed`: it
/// refuses while the game runs or starts (Minecraft rewrites servers.dat on
/// exit and would clobber this write) or while another writer holds the
/// instance, as `write_allowed` does, and while held it also keeps a launch
/// and a settings-sync join or stop out of the sync point that follows — both
/// of those write the same sidecar.
fn add_server_at(
    instance_id: &str,
    path: &Path,
    name: &str,
    address: &str,
    then: impl FnOnce(),
) -> Result<()> {
    validate_server_name(name)?;
    crate::launch::quick_play::validate_server_address(address)?;
    let claim = crate::instances::maintenance::claim_write(instance_id)?;
    let mut root = match fsio::read_if_present(path)? {
        Some(bytes) => nbt::parse(&bytes)?,
        None => nbt::empty_root(),
    };
    nbt::push_server(&mut root, name.trim(), address)?;
    fsio::write_atomic(path, &nbt::serialize(&root)?)?;
    then();
    drop(claim);
    Ok(())
}

/// [`remove_saved_server`] on a path, under the claim [`add_server_at`]
/// takes.
fn remove_server_at(
    instance_id: &str,
    path: &Path,
    index: usize,
    expected_address: &str,
    then: impl FnOnce(),
) -> Result<()> {
    let claim = crate::instances::maintenance::claim_write(instance_id)?;
    let Some(bytes) = fsio::read_if_present(path)? else {
        return Err(Error::SavedServerListChanged);
    };
    let mut root = nbt::parse(&bytes)?;
    nbt::remove_server(&mut root, index, expected_address)?;
    fsio::write_atomic(path, &nbt::serialize(&root)?)?;
    then();
    drop(claim);
    Ok(())
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

    #[test]
    fn an_edit_is_refused_while_the_instance_is_claimed() {
        let id = format!("servers-claimed-{}", crate::instances::ids::new_id());
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join(".minecraft").join("servers.dat");
        let claim = crate::instances::maintenance::claim_write(&id).unwrap();
        assert!(matches!(
            add_server_at(&id, &p, "A", "a.example.net", || {}),
            Err(Error::InstanceBusy)
        ));
        assert!(matches!(
            remove_server_at(&id, &p, 0, "a.example.net", || {}),
            Err(Error::InstanceBusy)
        ));
        assert!(!p.exists(), "nothing written under the claim");
        drop(claim);
        add_server_at(&id, &p, "A", "a.example.net", || {}).unwrap();
        assert_eq!(list_at(&p).unwrap().len(), 1);
        remove_server_at(&id, &p, 0, "a.example.net", || {}).unwrap();
        assert!(list_at(&p).unwrap().is_empty());
    }

    #[test]
    fn the_sync_after_an_edit_runs_under_the_edits_claim() {
        let id = format!("servers-then-{}", crate::instances::ids::new_id());
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("servers.dat");
        let held = |what: &str| {
            assert!(
                crate::instances::maintenance::claim_write(&id).is_err(),
                "{what}: the claim is held while the sync runs"
            );
        };
        add_server_at(&id, &p, "A", "a.example.net", || held("add")).unwrap();
        remove_server_at(&id, &p, 0, "a.example.net", || held("remove")).unwrap();
        crate::instances::maintenance::claim_write(&id).expect("released after");
    }

    #[test]
    fn a_servers_dat_that_cannot_be_read_is_an_error_not_an_empty_list() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("servers.dat");
        assert!(list_at(&p).unwrap().is_empty(), "absent is empty");
        std::fs::create_dir(&p).unwrap();
        assert!(list_at(&p).is_err());
    }
}
