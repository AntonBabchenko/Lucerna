pub mod accounts;
mod commands;
pub mod diag;
pub mod error;
pub mod forge;
pub mod instances;
pub mod jre;
pub mod launch;
pub mod logs;
pub mod mods;
pub mod network;
pub mod paths;
pub mod platform;
pub mod playtime;
pub mod process;
pub mod servers;
pub mod servers_runtime;
pub mod tray;
pub mod update;
pub mod verify;
pub mod versions;
pub mod window;
pub mod worlds;

/// Process-wide lock for tests that mutate `LUCERNA_EXTRA_ALLOWED_HOSTS`.
/// All wiremock-backed unit tests must hold this lock for the duration of
/// the test so that parallel threads don't race on the env-var state.
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

use tauri_specta::{collect_commands, collect_events, Builder};

/// Construct the tauri-specta builder (command + event registry). Extracted
/// from `run()` so the command/event registration lives in one reusable place
/// and bindings export can be driven from a single source of truth (the
/// `#[cfg(debug_assertions)]` export in `run()` calls this).
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::greet,
            commands::list_accounts,
            commands::get_active_account,
            commands::set_active_account,
            commands::remove_account,
            commands::add_offline_account,
            commands::begin_microsoft_signin,
            commands::refresh_microsoft_account,
            commands::account_skin,
            commands::list_versions,
            commands::install_version,
            commands::install_instance,
            commands::launch_instance,
            commands::instance_quick_play_support,
            commands::stop_minecraft,
            commands::list_log_files,
            commands::read_log_file,
            commands::latest_crash,
            commands::diagnose_log,
            commands::diagnose_latest,
            commands::build_repair_plan,
            commands::execute_repair,
            commands::share_log_to_mclogs,
            commands::open_mods_folder,
            commands::list_worlds,
            commands::list_world_names,
            commands::backup_world,
            commands::list_backups,
            commands::restore_backup,
            commands::delete_backup,
            commands::delete_world,
            commands::open_saves_folder,
            commands::open_backups_folder,
            commands::world_import,
            commands::list_saved_servers,
            commands::add_saved_server,
            commands::remove_saved_server,
            commands::open_log_folder,
            commands::delete_log_file,
            commands::clear_old_logs,
            commands::apply_log_retention,
            commands::list_fabric_loaders,
            commands::list_quilt_loaders,
            commands::list_forge_loaders,
            commands::list_neoforge_loaders,
            // Multi-instance (v0.3.0):
            commands::list_instances,
            commands::get_active_instance,
            commands::set_active_instance,
            commands::create_instance,
            commands::delete_instance,
            commands::set_instance_name,
            commands::change_instance_mc,
            commands::set_instance_loader,
            commands::set_instance_memory,
            commands::set_instance_jvm_args,
            commands::instance_memory_bounds,
            commands::detach_instance_pack,
            commands::open_instance_folder,
            commands::verify_instance,
            commands::repair_instance,
            commands::get_playtime,
            commands::window_set_compact,
            commands::window_set_expanded_floor,
            // Mod browser (v0.5.0 sub-feature 3):
            commands::mods_search,
            commands::mods_project,
            commands::mods_projects,
            commands::mods_versions,
            commands::mods_filter_satisfying,
            commands::mods_resolve_deps,
            commands::mods_resolve_install_plan,
            commands::mods_install_with_deps,
            commands::mods_list_installed,
            commands::mods_disable,
            commands::mods_enable,
            commands::mods_uninstall,
            commands::mods_check_updates,
            commands::asset_install,
            commands::assets_list,
            commands::asset_uninstall,
            commands::asset_install_local,
            commands::assets_check_updates,
            commands::check_instance_mod_compat,
            commands::scan_instance_mod_compat,
            commands::mods_pack_origin_summary,
            commands::mods_enrich_pack_mods,
            commands::mods_update_one,
            commands::mods_find_orphans,
            commands::mods_dependency_graph,
            commands::instance_dependency_preflight,
            commands::mods_install_missing_required,
            commands::mods_inspect_local,
            commands::mods_install_local,
            commands::mods_get_curseforge_key_status,
            commands::mods_set_curseforge_key,
            commands::mods_clear_curseforge_key,
            commands::mods_cache_size_bytes,
            commands::mods_clear_cache,
            // Modpack import (v0.5.0 sub-feature 4):
            commands::modpack_inspect,
            commands::modpack_import,
            commands::modpack_search,
            commands::modpack_fetch_to_temp,
            commands::modpack_status,
            commands::modpack_resolve_missing_with,
            commands::modpack_restore_file,
            commands::modpack_get_versions,
            commands::modpack_project,
            commands::modpack_source_caps,
            commands::modpack_update_status,
            commands::modpacks_check_updates,
            commands::modpack_compute_update,
            commands::modpack_apply_update,
            commands::modpack_reimport_overrides,
            // Modpack export (v0.6.0):
            commands::export_preview,
            commands::export_modpack,
            // Launcher instance import (Phase 1):
            commands::launcher_import_discover,
            commands::launcher_import_inspect_folder,
            commands::launcher_import_run,
            commands::open_imported_source_folder,
            // Onboarding (v0.5.0 sub-feature 5):
            commands::app_settings_get,
            commands::app_settings_mark_tour_completed,
            commands::app_settings_set_general,
            commands::gpu_capability,
            // Self-update:
            commands::update_check,
            commands::update_install,
            commands::update_dismiss,
            // Own server (Plan 1: vanilla create / list / delete):
            commands::server_create,
            commands::server_list,
            commands::server_delete,
            commands::server_rename,
            commands::server_update_runtime_config,
            // Own server (Plan 2: process management):
            commands::server_start,
            commands::server_stop,
            commands::server_restart,
            commands::server_send_command,
            // Own server (Plan 3: properties read/write):
            commands::server_read_properties,
            commands::server_write_properties,
            // Own server (Plan 3: mod management + folder open):
            commands::server_list_mods,
            commands::server_delete_mod,
            commands::server_open_folder,
            // Own server (Plan 4: diagnosis + repair):
            commands::server_diagnose,
            commands::server_remove_mods,
            commands::server_accept_eula,
            commands::server_stop_orphan,
            commands::server_change_port,
            // Own server (Plan 4 phase 2: class-B log fixes):
            commands::server_raise_heap,
            commands::server_lower_heap,
            commands::server_redownload_jar,
            commands::server_disable_mods,
            commands::server_install_missing_dep,
            commands::server_quarantine_client_mods,
            // Own server (Plan 5: SFTP upload + export):
            commands::server_set_upload_config,
            commands::server_upload,
            commands::server_export_zip,
            // Own server (S4 #24/#28: host-key preview + SFTP auth method):
            commands::server_host_key_preview,
            commands::server_get_upload_auth,
            commands::server_set_upload_auth,
            // Own server (Plan 6: client instance from server):
            commands::server_create_client_instance,
            // Own server (Plan 7: import from zip/folder):
            commands::server_import_inspect,
            commands::server_import_commit,
            commands::server_import_cancel,
            // Own server (Plan 8: connectivity / friends-join):
            commands::server_connectivity,
            commands::server_public_address,
            // Own server (Plan 9: backups):
            commands::server_backup_create,
            commands::server_backup_list,
            commands::server_backup_restore,
            commands::server_backup_delete,
            commands::server_backup_policy_get,
            commands::server_backup_policy_set,
            // Own server (Plan 10: log retention):
            commands::server_list_logs,
            commands::server_read_log,
            commands::server_open_logs_folder,
            // Own server (Plan 11: firewall help):
            commands::server_firewall_status,
            commands::server_firewall_add_rule,
            // Own server (S4 #9: whitelist / ops editor):
            commands::server_whitelist_list,
            commands::server_whitelist_add,
            commands::server_whitelist_remove,
            commands::server_ops_list,
            commands::server_ops_add,
            commands::server_ops_remove,
        ])
        .events(collect_events![
            network::DownloadProgress,
            versions::InstallProgress,
            verify::VerifyProgress,
            launch::ProcessSpawned,
            launch::ProcessExited,
            commands::ModInstallProgress,
            commands::ModInstalled,
            commands::ModUninstalled,
            commands::ModToggle,
            commands::ModInstallFailed,
            commands::GpuPrefApplied,
            servers_runtime::runtime::ServerLogLine,
            servers_runtime::runtime::ServerSpawned,
            servers_runtime::runtime::ServerExited,
            servers_runtime::transfer::ServerUploadProgress,
        ])
}

/// Dev-only: handle `--export-bindings [--check]` passed to the launcher.
///
/// Regenerates `src/lib/ipc/bindings.ts` from the same `specta_builder()` the
/// app uses, without launching the app — the no-dev-server regen path. Returns
/// `Some(exit_code)` when the flag is present (the caller should exit), or
/// `None` to continue launching normally.
///
/// This lives on the launcher binary rather than a second `[[bin]]`: a second
/// binary in this package makes `tauri build` try to copy a binary the bundler
/// can't find and aborts the bundle on every platform (it copies every declared
/// package binary), and only this binary carries the Windows application
/// manifest the exporter needs to start (a `tests/`/`examples/` binary does not
/// and fails with STATUS_ENTRYPOINT_NOT_FOUND).
#[cfg(debug_assertions)]
fn maybe_export_bindings_from_args(builder: &Builder<tauri::Wry>) -> Option<i32> {
    use specta_typescript::Typescript;

    if !std::env::args().any(|a| a == "--export-bindings") {
        return None;
    }
    let target = "../src/lib/ipc/bindings.ts";

    if std::env::args().any(|a| a == "--check") {
        let tmp = std::env::temp_dir().join("lucerna-bindings-freshness-check.ts");
        if let Err(e) = builder.export(Typescript::default(), &tmp) {
            crate::diag!("Failed to export bindings for --check: {e}");
            return Some(1);
        }
        let fresh = std::fs::read_to_string(&tmp).ok();
        let committed = std::fs::read_to_string(target).ok();
        if fresh.is_some() && fresh == committed {
            println!("bindings.ts is up to date.");
            return Some(0);
        }
        crate::diag!(
            "bindings.ts is STALE — a Rust command/type changed without regenerating.\n\
             Run: cargo run --manifest-path src-tauri/Cargo.toml -- --export-bindings"
        );
        return Some(1);
    }

    match builder.export(Typescript::default(), target) {
        Ok(()) => {
            println!("Regenerated {target}");
            Some(0)
        }
        Err(e) => {
            crate::diag!("Failed to regenerate bindings: {e}");
            Some(1)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    // Dev-only no-app bindings regen: `cargo run -- --export-bindings [--check]`
    // exports the bindings and exits before the app launches.
    #[cfg(debug_assertions)]
    if let Some(code) = maybe_export_bindings_from_args(&builder) {
        std::process::exit(code);
    }

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/lib/ipc/bindings.ts",
        )
        // Dev-only: if the bindings file cannot be written, the dev tree is
        // unwritable and there's no graceful path forward — crash loudly.
        .expect("Failed to export TypeScript bindings");

    tauri::Builder::default()
        // Single-instance guard MUST be the first plugin (Tauri requirement):
        // when a second copy of Lucerna is launched, this callback runs in the
        // already-running process and the new one exits. We surface the existing
        // window via the tray-restore path, which shows + unminimizes + focuses
        // it (and clears the tray icon if the launcher was hidden to tray during
        // a Minecraft session). argv/cwd of the second launch are unused — we
        // only ever want to bring the running window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Err(e) = crate::tray::restore_from_tray(app) {
                crate::diag!("[single-instance] failed to restore window: {e}");
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .manage(window::WindowSizeState::default())
        .setup(move |app| {
            // Open the launcher's own diagnostic log (lucerna.log) first, so
            // subsequent `diag!` lines in setup are captured. Best-effort.
            diag::init(app.handle());

            // Windows: the taskbar / title-bar icon is a single HICON that the
            // shell rescales per context. Tauri's default window icon is built
            // from the largest configured frame, so at a 24px (100% DPI) taskbar
            // slot our pixel-art lantern ends up downscaled from 256 and looks
            // blurry. Set a crisp native-size (24px) icon so it renders 1:1.
            #[cfg(windows)]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    const ICON_RGBA: &[u8] = include_bytes!("../icons/window-icon.rgba");
                    if let Err(e) = win.set_icon(tauri::image::Image::new(ICON_RGBA, 24, 24)) {
                        crate::diag!("[setup] set window icon failed: {e}");
                    }
                }
            }

            // One-shot instance migration. Non-fatal on error — the UI has
            // an empty-state fallback that lets the user manually recover
            // by creating an instance through the Manage modal.
            if let Err(e) = instances::migrate::migrate_or_seed(app.handle()) {
                crate::diag!("[setup] instances::migrate_or_seed failed: {e}");
            }
            builder.mount_events(app);

            // Idle refresh task: every 60s, scan accounts and refresh any
            // Microsoft account whose access token is within 5 minutes of expiry.
            // Mirrors Mojang reference launcher's silent-renew behaviour. Failures
            // are logged to stderr but don't surface to the UI — the next interactive
            // sign-in will prompt the user if the refresh chain is unrecoverable.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    let accounts = match crate::accounts::list_accounts(&app_handle) {
                        Ok(xs) => xs,
                        Err(e) => {
                            crate::diag!("microsoft refresh: list_accounts failed: {e}");
                            continue;
                        }
                    };
                    for a in accounts {
                        if a.kind != crate::accounts::store::AccountKind::Microsoft {
                            continue;
                        }
                        let Some(exp) = a.expires_at else {
                            continue;
                        };
                        if exp <= now + 300.0 {
                            let res = crate::accounts::microsoft::refresh(&app_handle, &a.id).await;
                            if let Err(e) = res {
                                crate::diag!("microsoft refresh: failed for account {}: {e}", a.id);
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            // Bug A root fix: never orphan server children. On launcher exit,
            // synchronously force-kill every tracked server process.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                crate::servers_runtime::runtime::kill_all_running();
            }
        });
}
