//! Reverse of "server from instance": build a client instance that mirrors a
//! launcher-owned server (same MC version + loader + mods), optionally
//! pre-registering the server in the instance's multiplayer list. The server
//! is read-only; nothing under it is written.

use crate::error::{Error, Result};
use crate::instances::schema::InstanceWithStatus;
use serde::Serialize;
use specta::Type;

/// IPC result of building a client instance from a server.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ClientInstanceResult {
    pub instance: InstanceWithStatus,
    /// Whether the server was added to the instance's multiplayer list.
    /// `false` when the user opted out, or the best-effort write failed.
    pub multiplayer_added: bool,
}

/// Build a client instance mirroring a launcher-owned server: same MC version
/// and loader, the server's mods copied in, optionally the server
/// pre-registered in the instance's multiplayer list. `modrinth_base`,
/// `cf_base`, and `cf_key` feed the best-effort mod-identity enrich pass
/// (same as launcher import).
#[allow(clippy::too_many_arguments)]
pub async fn create_client_instance(
    app: &tauri::AppHandle,
    server_id: &str,
    name: &str,
    add_to_multiplayer: bool,
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
) -> Result<ClientInstanceResult> {
    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, server_id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;

    // A Paper/Purpur server produces a *vanilla* client instance (plugins are
    // server-only): widen to the client loader, dropping any plugin-core build
    // string from loader_version. Mod loaders map through unchanged.
    let client_loader = file
        .loader
        .as_loader_kind()
        .unwrap_or(crate::instances::schema::LoaderKind::Vanilla);
    let client_loader_version = if file.loader.plugin_capable() {
        None
    } else {
        file.loader_version.clone()
    };

    // Mirror version + loader; heap/jvm get fresh adaptive defaults via
    // create_instance. Record the source server for provenance.
    let created = crate::instances::create_instance(
        app,
        name.to_string(),
        file.mc_version.clone(),
        client_loader,
        client_loader_version,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(server_id.to_string()),
    )?;
    let instance_id = created.id.clone();

    let instance_root = crate::paths::instance_dir(app, &instance_id)
        .map_err(|e| Error::io("<instance_dir>", e))?;
    let dst_mods = crate::paths::minecraft_dir(app, &instance_id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?
        .join("mods");

    // Mods are mandatory for a mod-core client: a copy failure rolls back the
    // half-built instance (mirrors the launcher-import rollback).
    // copy_instance_mods preserves .jar and .jar.disabled state and treats a
    // missing source as 0 copied. A Paper/Purpur server holds Bukkit plugins
    // (server-only), so its vanilla client instance copies no mods.
    if !file.loader.plugin_capable() {
        if let Err(e) = crate::servers_runtime::create::copy_instance_mods(&p.mods, &dst_mods) {
            // Remove the half-built instance directly (not via delete_instance,
            // which silently no-ops on the last instance) — see pipeline.rs's
            // run_import rollback for the full rationale.
            let _ = std::fs::remove_dir_all(&instance_root);
            return Err(e);
        }
    }

    // Mirror the server's non-mod configuration so the produced client matches
    // it (#17): `config/`, `resourcepacks/`, `datapacks/`. Without these the
    // client can desync from the server (e.g. a config-driven mod crashes on
    // join) — the very failure this "client from server" feature exists to
    // avoid. Best-effort: the instance is already usable from the mandatory mod
    // copy above, so a partial config copy is logged, not a rollback.
    let mc_dir = crate::paths::minecraft_dir(app, &instance_id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?;
    if let Err(e) = copy_server_config_dirs(&p.runtime, &mc_dir) {
        crate::diag!("instance-from-server: config-dir copy incomplete: {e}");
    }

    // Register + best-effort enrich the copied jars (no manifest identities).
    // Non-fatal: the mods are already on disk; only the Mods-tab metadata is
    // affected if this fails.
    let _ = crate::instances::import::pipeline::adopt_copied_jars(
        &instance_root,
        &dst_mods,
        &[],
        modrinth_base,
        cf_base,
        cf_key,
    )
    .await;

    // Optionally pre-register the server in the instance's multiplayer list.
    let mut multiplayer_added = false;
    if add_to_multiplayer {
        let address = address_for_port(crate::servers_runtime::runtime::read_port(&p.runtime));
        match crate::servers::add_saved_server(app, &instance_id, &file.name, &address) {
            Ok(()) => multiplayer_added = true,
            Err(e) => crate::diag!("instance-from-server: add_saved_server failed: {e}"),
        }
    }

    Ok(ClientInstanceResult {
        instance: created,
        multiplayer_added,
    })
}

/// Multiplayer address for the saved-servers entry. A launcher server runs
/// locally, so the host is always `localhost`; the port comes from the
/// server's `server.properties` (`None` → Mojang's default 25565).
pub(crate) fn address_for_port(port: Option<u16>) -> String {
    format!("localhost:{}", port.unwrap_or(25565))
}

/// Server runtime subdirectories that carry client-relevant configuration and
/// must be mirrored into the produced client instance (#17).
const CONFIG_DIRS: &[&str] = &["config", "resourcepacks", "datapacks"];

/// Copy the server's `config/`, `resourcepacks/`, and `datapacks/` directories
/// (when present) from `runtime` into the client instance's `.minecraft` dir.
/// Skips absent dirs and symlinks (cycle/escape safety). The first hard error
/// surfaces to the caller, which logs it (best-effort by design).
pub(crate) fn copy_server_config_dirs(
    runtime: &std::path::Path,
    mc_dir: &std::path::Path,
) -> Result<()> {
    for name in CONFIG_DIRS {
        let src = runtime.join(name);
        if src.is_dir() {
            copy_dir_recursive(&src, &mc_dir.join(name))?;
        }
    }
    Ok(())
}

/// Recursive directory copy that does not follow symlinks. Creates `dst` and
/// merges into it (existing files are overwritten — the server's copy wins,
/// which is the intended fidelity behaviour).
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst.display().to_string(), e))?;
    for entry in std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(src.display().to_string(), e))?;
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            continue;
        }
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &to).map_err(|e| Error::io(to.display().to_string(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_uses_explicit_port() {
        assert_eq!(address_for_port(Some(25570)), "localhost:25570");
    }

    #[test]
    fn address_defaults_to_25565_when_absent() {
        assert_eq!(address_for_port(None), "localhost:25565");
    }

    #[test]
    fn config_dirs_copied_into_client_instance() {
        use std::fs;
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let mc = d.path().join("minecraft");
        fs::create_dir_all(runtime.join("config/fabric")).unwrap();
        fs::write(runtime.join("config/fabric/sodium.json"), b"{}").unwrap();
        fs::create_dir_all(runtime.join("resourcepacks")).unwrap();
        fs::write(runtime.join("resourcepacks/pack.zip"), b"z").unwrap();
        fs::create_dir_all(runtime.join("datapacks")).unwrap();
        fs::write(runtime.join("datapacks/vanilla.zip"), b"d").unwrap();
        // A dir we must NOT copy (only the three config dirs are mirrored).
        fs::create_dir_all(runtime.join("logs")).unwrap();
        fs::write(runtime.join("logs/latest.log"), b"noise").unwrap();

        copy_server_config_dirs(&runtime, &mc).unwrap();

        assert_eq!(
            fs::read(mc.join("config/fabric/sodium.json")).unwrap(),
            b"{}"
        );
        assert!(mc.join("resourcepacks/pack.zip").is_file());
        assert!(mc.join("datapacks/vanilla.zip").is_file());
        assert!(!mc.join("logs").exists(), "only config dirs are mirrored");
    }

    #[test]
    fn config_dir_copy_is_noop_when_runtime_has_none() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        let mc = d.path().join("minecraft");
        // No config/resourcepacks/datapacks present → Ok, nothing created.
        copy_server_config_dirs(&runtime, &mc).unwrap();
        assert!(!mc.join("config").exists());
    }

    #[test]
    fn client_instance_result_serializes_multiplayer_flag() {
        use crate::instances::schema::{InstanceFile, InstanceWithStatus, LoaderKind};
        let file = InstanceFile {
            id: "i1".into(),
            name: "Client".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 4096,
            min_heap_mb: None,
            extra_jvm_args: String::new(),
            created_unix_ms: 1.0,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from: None,
            created_from_server: Some("srv-1".into()),
            handled_log_sig: None,
        };
        let res = ClientInstanceResult {
            instance: InstanceWithStatus::from_file(&file, false, false),
            multiplayer_added: true,
        };
        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains(r#""multiplayer_added":true"#), "got: {json}");
        assert!(
            json.contains(r#""created_from_server":"srv-1""#),
            "got: {json}"
        );
    }
}
