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
        None, // heap: adaptive default
        crate::instances::schema::PackOrigin::default(),
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
    // it (#17): `config/`, `resourcepacks/`. Without these the
    // client can desync from the server (e.g. a config-driven mod crashes on
    // join) — the very failure this "client from server" feature exists to
    // avoid. Best-effort: the instance is already usable from the mandatory mod
    // copy above, so a partial config copy is logged, not a rollback.
    let mc_dir = crate::paths::minecraft_dir(app, &instance_id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?;
    if let Err(e) = copy_server_config_dirs(&p.runtime, &mc_dir) {
        crate::diag!("instance-from-server: config-dir copy incomplete: {e}");
    }

    // Datapacks are NOT part of that mirror: they belong in the instance's
    // datapack LIBRARY with their catalogue identity, not in a directory under
    // `.minecraft/`. See `copy_server_datapacks`.
    copy_server_datapacks(&p.runtime, &instance_root).await;

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
const CONFIG_DIRS: &[&str] = &["config", "resourcepacks"];

/// Copy the server's `config/` and `resourcepacks/` directories (when present)
/// from `runtime` into the client instance's `.minecraft` dir. Skips absent
/// dirs and symlinks (cycle/escape safety). The first hard error surfaces to
/// the caller, which logs it (best-effort by design).
///
/// `datapacks/` used to be on this list and was dead at both ends: a server
/// keeps its packs in `runtime/<level>/datapacks/`, and the client reads its
/// library at `<instance>/datapacks/`, never `.minecraft/datapacks/`. They are
/// carried by [`copy_server_datapacks`] instead.
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

/// Carry the server's world datapacks into the produced instance's datapack
/// library.
///
/// NOT a directory copy. The client's model is a library entry at
/// `<instance>/datapacks/` plus a registry row, with a hardlink into each
/// world that uses the pack; copying files into `.minecraft/datapacks/` would
/// put them where nothing reads them. The produced instance has no worlds at
/// all — this command never copies `saves/` — so the library is not a
/// compromise destination, it is the only one, and the world picker already
/// explains that state when the user goes to place a pack.
///
/// Provenance carries wherever the server's sidecar has it, so a pack the
/// admin installed from a catalogue stays update-checkable on the client. A
/// pack with no identity installs without one rather than with a guess.
///
/// The server's enabled/disabled state is deliberately NOT carried: it lives
/// in that world's `level.dat`, and the target has no world to hold it.
///
/// Best-effort, like the config mirror above: the instance is already usable
/// from the mandatory mod copy, so a pack that fails is logged and the rest
/// proceed.
pub(crate) async fn copy_server_datapacks(
    runtime: &std::path::Path,
    instance_root: &std::path::Path,
) {
    let props = std::fs::read_to_string(runtime.join("server.properties")).unwrap_or_default();
    let world = crate::servers_runtime::datapacks::world_dir(runtime, &props);
    let dp_dir = world.join("datapacks");

    for entry in crate::servers_runtime::datapacks::listing::entries(&world) {
        // A `level.dat` name whose file is gone has nothing to install. This
        // is for the LOG, not for correctness: without it the install below
        // fails on the missing file and writes no row either way, but every
        // ghost would leave an alarming "not carried" line about a pack that
        // was never there. Verified by mutation — removing this changes no
        // test outcome.
        if !entry.present {
            continue;
        }
        let r = &entry.record;
        let src = dp_dir.join(&r.filename);
        let provenance = match (r.source, r.project_id.clone(), r.version_id.clone()) {
            (Some(source), Some(project_id), Some(version_id)) => {
                Some(crate::datapacks::DatapackProvenance {
                    source,
                    project_id,
                    version_id,
                    version_number: r.version_number.clone(),
                })
            }
            _ => None,
        };

        let res = match &provenance {
            // Identity-bearing: place the bytes under the same name, keeping
            // the identity that makes update checking possible.
            Some(p) => match tokio::fs::read(&src).await {
                Ok(bytes) => {
                    crate::datapacks::library::install_named_at(instance_root, &r.filename, &bytes, Some(p))
                        .await
                        .map(|_| ())
                }
                Err(e) => Err(Error::io(src.display().to_string(), e)),
            },
            // No identity: `install_local_at` already handles both a `.zip`
            // file and a FOLDER pack — it zips a folder in memory under
            // `<folder>.zip` — so folder packs need no special case here.
            None => crate::datapacks::library::install_local_at(instance_root, &src)
                .await
                .map(|_| ()),
        };
        if let Err(e) = res {
            crate::diag!(
                "instance-from-server: datapack {} not carried: {e}",
                r.filename
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- datapack carry-over fixtures -------------------------------------
    // A server world at `<runtime>/<level>/`, its packs in `datapacks/` under
    // it, and the provenance sidecar beside them in the WORLD dir (not in
    // `datapacks/` — see servers_runtime::datapacks).

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::{Cursor, Write};
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(body).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    const PACK_MCMETA: &[u8] = br#"{"pack":{"pack_format":15,"description":"x"}}"#;

    fn datapack_zip() -> Vec<u8> {
        zip_of(&[
            ("pack.mcmeta", PACK_MCMETA),
            ("data/ns/tags/foo.json", b"{}"),
        ])
    }

    /// A server world with `packs` written into its `datapacks/` dir.
    fn server_world(runtime: &std::path::Path, level: &str) -> std::path::PathBuf {
        let world = runtime.join(level);
        std::fs::create_dir_all(world.join("datapacks")).unwrap();
        world
    }

    /// Plant a provenance row for `filename` in the world's sidecar.
    fn write_sidecar_row(world: &std::path::Path, filename: &str, project: (&str, &str, &str)) {
        use crate::servers_runtime::installed::{save, ServerInstalledRecord};
        let (source, project_id, version_id) = project;
        let source = match source {
            "modrinth" => crate::mods::platform::ModSource::Modrinth,
            _ => crate::mods::platform::ModSource::Curseforge,
        };
        save(
            world,
            &[ServerInstalledRecord {
                filename: filename.to_string(),
                sha1: String::new(),
                source: Some(source),
                project_id: Some(project_id.to_string()),
                version_id: Some(version_id.to_string()),
                name: None,
                version_number: Some("1.2.3".to_string()),
                enrich_attempted: true,
            }],
        )
        .unwrap();
    }

    async fn library_rows(
        instance_root: &std::path::Path,
    ) -> Vec<crate::datapacks::InstalledDatapack> {
        crate::datapacks::registry::list(instance_root).await.unwrap()
    }

    /// A `.zip` with a catalogue identity arrives in the instance library WITH
    /// that identity — otherwise the produced instance could never check the
    /// pack for updates, which is the whole reason to carry it rather than
    /// copy bytes.
    #[tokio::test]
    async fn a_catalogue_pack_arrives_in_the_library_with_its_provenance() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        let world = server_world(&runtime, "world");
        std::fs::write(world.join("datapacks/terralith.zip"), datapack_zip()).unwrap();
        write_sidecar_row(&world, "terralith.zip", ("modrinth", "abc123", "v9"));

        copy_server_datapacks(&runtime, &inst).await;

        let rows = library_rows(&inst).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "terralith.zip");
        assert_eq!(rows[0].project_id.as_deref(), Some("abc123"));
        assert_eq!(rows[0].version_id.as_deref(), Some("v9"));
    }

    /// A hand-dropped pack has no identity to carry. It still arrives — with
    /// no provenance, which is honest, rather than with a guessed one.
    #[tokio::test]
    async fn a_hand_dropped_pack_arrives_without_provenance() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        let world = server_world(&runtime, "world");
        std::fs::write(world.join("datapacks/mine.zip"), datapack_zip()).unwrap();

        copy_server_datapacks(&runtime, &inst).await;

        let rows = library_rows(&inst).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "mine.zip");
        assert!(rows[0].project_id.is_none());
    }

    /// A folder pack is a legal datapack. The client library stores zips, so
    /// it arrives zipped under `<folder>.zip` — exactly what a local folder
    /// install produces.
    #[tokio::test]
    async fn a_folder_pack_arrives_zipped_under_its_folder_name() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        let world = server_world(&runtime, "world");
        let pack = world.join("datapacks/my pack");
        std::fs::create_dir_all(pack.join("data/ns/tags")).unwrap();
        std::fs::write(pack.join("pack.mcmeta"), PACK_MCMETA).unwrap();
        std::fs::write(pack.join("data/ns/tags/foo.json"), b"{}").unwrap();

        copy_server_datapacks(&runtime, &inst).await;

        let rows = library_rows(&inst).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "my pack.zip");
    }

    /// One unusable pack must not cost the user the others: the produced
    /// instance is already usable from the mandatory mod copy, so this step is
    /// best-effort by design.
    #[tokio::test]
    async fn one_bad_pack_does_not_stop_the_others() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        let world = server_world(&runtime, "world");
        std::fs::write(world.join("datapacks/broken.zip"), b"not a zip").unwrap();
        std::fs::write(world.join("datapacks/good.zip"), datapack_zip()).unwrap();

        copy_server_datapacks(&runtime, &inst).await;

        let rows = library_rows(&inst).await;
        assert_eq!(rows.len(), 1, "the good pack still carried");
        assert_eq!(rows[0].filename, "good.zip");
    }

    /// A server whose world has never booted has no `level.dat`. Its packs
    /// still carry.
    #[tokio::test]
    async fn packs_carry_from_a_world_that_never_booted() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        let world = server_world(&runtime, "world");
        std::fs::write(world.join("datapacks/pack.zip"), datapack_zip()).unwrap();
        assert!(!world.join("level.dat").exists());

        copy_server_datapacks(&runtime, &inst).await;

        assert_eq!(library_rows(&inst).await.len(), 1);
    }

    /// The `level-name` from `server.properties` is honoured — packs live
    /// under the level the server actually uses, not a hardcoded `world`.
    #[tokio::test]
    async fn the_level_name_from_server_properties_is_honoured() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(runtime.join("server.properties"), "level-name=my_realm\n").unwrap();
        let world = server_world(&runtime, "my_realm");
        std::fs::write(world.join("datapacks/pack.zip"), datapack_zip()).unwrap();
        // A decoy under the default level name must NOT be picked up.
        let decoy = server_world(&runtime, "world");
        std::fs::write(decoy.join("datapacks/decoy.zip"), datapack_zip()).unwrap();

        copy_server_datapacks(&runtime, &inst).await;

        let rows = library_rows(&inst).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "pack.zip");
    }

    /// A `level.dat` entry whose file is gone has nothing to install. It must
    /// not error, and must not fabricate a library row for a pack that does
    /// not exist.
    #[tokio::test]
    async fn a_ghost_entry_is_skipped_without_an_error() {
        use crate::datapacks::{level_dat, level_dat_entry};
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        let world = server_world(&runtime, "world");
        // level.dat names a pack whose file was deleted by hand.
        let mut root = fastnbt::Value::Compound(std::collections::HashMap::new());
        level_dat::set_enabled(&mut root, &level_dat_entry("gone.zip"), true).unwrap();
        level_dat::write_at(&world, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();
        // …plus a real one, so the test proves selectivity rather than
        // "nothing was carried at all".
        std::fs::write(world.join("datapacks/real.zip"), datapack_zip()).unwrap();

        copy_server_datapacks(&runtime, &inst).await;

        let rows = library_rows(&inst).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "real.zip");
    }

    /// A server with no world dir at all is a no-op, not an error.
    #[tokio::test]
    async fn a_server_with_no_world_carries_nothing() {
        let d = tempfile::tempdir().unwrap();
        let runtime = d.path().join("runtime");
        let inst = d.path().join("instance");
        std::fs::create_dir_all(&runtime).unwrap();

        copy_server_datapacks(&runtime, &inst).await;

        assert!(library_rows(&inst).await.is_empty());
    }

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
        // `runtime/datapacks/` is NOT where a server keeps its packs — those
        // live in `runtime/<level>/datapacks/` — and `.minecraft/datapacks/`
        // is not somewhere the client reads: its library is
        // `<instance>/datapacks/`. The mirror was dead at both ends, so it is
        // gone; datapacks are carried by `copy_server_datapacks` instead.
        assert!(
            !mc.join("datapacks").exists(),
            "datapacks go to the instance library, not into .minecraft/"
        );
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
            uid: None,
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
