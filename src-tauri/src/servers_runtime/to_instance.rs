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

    // Mirror version + loader; heap/jvm get fresh adaptive defaults via
    // create_instance. Record the source server for provenance.
    let created = crate::instances::create_instance(
        app,
        name.to_string(),
        file.mc_version.clone(),
        file.loader,
        file.loader_version.clone(),
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

    // Mods are mandatory: a copy failure rolls back the half-built instance
    // (mirrors the launcher-import rollback). copy_instance_mods preserves
    // .jar and .jar.disabled state and treats a missing source as 0 copied.
    if let Err(e) = crate::servers_runtime::create::copy_instance_mods(&p.mods, &dst_mods) {
        // Remove the half-built instance directly (not via delete_instance,
        // which silently no-ops on the last instance) — see pipeline.rs's
        // run_import rollback for the full rationale.
        let _ = std::fs::remove_dir_all(&instance_root);
        return Err(e);
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
    fn client_instance_result_serializes_multiplayer_flag() {
        use crate::instances::schema::{InstanceFile, InstanceWithStatus, LoaderKind};
        let file = InstanceFile {
            id: "i1".into(),
            name: "Client".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 4096,
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
            instance: InstanceWithStatus::from_file(&file, false),
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
