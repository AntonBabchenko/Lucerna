pub mod accounts;
mod commands;
pub mod error;
pub mod forge;
pub mod instances;
pub mod jre;
pub mod launch;
pub mod logs;
pub mod mods;
pub mod network;
pub mod paths;
pub mod playtime;
pub mod process;
pub mod tray;
pub mod update;
pub mod versions;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::greet,
            commands::list_accounts,
            commands::get_active_account,
            commands::set_active_account,
            commands::remove_account,
            commands::add_offline_account,
            commands::begin_microsoft_signin,
            commands::refresh_microsoft_account,
            commands::list_versions,
            commands::install_version,
            commands::install_instance,
            commands::launch_instance,
            commands::stop_minecraft,
            commands::list_log_files,
            commands::read_log_file,
            commands::latest_crash,
            commands::diagnose_log,
            commands::share_log_to_mclogs,
            commands::open_mods_folder,
            commands::list_worlds,
            commands::backup_world,
            commands::list_backups,
            commands::restore_backup,
            commands::delete_backup,
            commands::delete_world,
            commands::open_saves_folder,
            commands::open_backups_folder,
            commands::open_log_folder,
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
            commands::detach_instance_pack,
            commands::open_instance_folder,
            commands::get_playtime,
            // Mod browser (v0.5.0 sub-feature 3):
            commands::mods_search,
            commands::mods_project,
            commands::mods_versions,
            commands::mods_resolve_deps,
            commands::mods_install_with_deps,
            commands::mods_list_installed,
            commands::mods_disable,
            commands::mods_enable,
            commands::mods_uninstall,
            commands::mods_check_updates,
            commands::check_instance_mod_compat,
            commands::mods_pack_origin_summary,
            commands::mods_enrich_pack_mods,
            commands::mods_update_one,
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
            commands::modpack_restore_file,
            commands::modpack_get_versions,
            commands::modpack_project,
            commands::modpack_check_update,
            commands::modpack_compute_update,
            commands::modpack_apply_update,
            commands::modpack_reimport_overrides,
            // Modpack export (v0.6.0):
            commands::export_preview,
            commands::export_modpack,
            // Onboarding (v0.5.0 sub-feature 5):
            commands::app_settings_get,
            commands::app_settings_mark_tour_completed,
            commands::app_settings_set_general,
            // Self-update:
            commands::update_check,
            commands::update_install,
            commands::update_dismiss,
        ])
        .events(collect_events![
            network::DownloadProgress,
            versions::InstallProgress,
            launch::ProcessSpawned,
            launch::ProcessExited,
            commands::ModInstallProgress,
            commands::ModInstalled,
            commands::ModUninstalled,
            commands::ModToggle,
            commands::ModInstallFailed,
        ]);

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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            // One-shot instance migration. Non-fatal on error — the UI has
            // an empty-state fallback that lets the user manually recover
            // by creating an instance through the Manage modal.
            if let Err(e) = instances::migrate::migrate_or_seed(app.handle()) {
                eprintln!("[setup] instances::migrate_or_seed failed: {e}");
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
                            eprintln!("microsoft refresh: list_accounts failed: {e}");
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
                                eprintln!("microsoft refresh: failed for account {}: {e}", a.id);
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        // Tauri's run() only returns on a fatal init failure (e.g., missing
        // webview runtime). There's nothing to recover to — crash loudly.
        .expect("error while running tauri application");
}
