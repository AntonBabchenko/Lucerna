mod commands;
pub mod accounts;
pub mod error;
pub mod forge;
pub mod instances;
pub mod jre;
pub mod launch;
pub mod logs;
pub mod mods;
pub mod network;
pub mod paths;
pub mod process;
pub mod versions;

/// Process-wide lock for tests that mutate `FTLAUNCHER_EXTRA_ALLOWED_HOSTS`.
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
            commands::list_versions,
            commands::install_version,
            commands::install_instance,
            commands::launch_instance,
            commands::stop_minecraft,
            commands::list_log_files,
            commands::read_log_file,
            commands::latest_crash,
            commands::open_mods_folder,
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
            commands::set_instance_version,
            commands::set_instance_loader,
            commands::set_instance_memory,
            commands::set_instance_jvm_args,
            commands::open_instance_folder,
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
            commands::mods_pack_origin_summary,
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
            commands::modpack_check_update,
            commands::modpack_compute_update,
            commands::modpack_apply_update,
            commands::modpack_reimport_overrides,
            // Onboarding (v0.5.0 sub-feature 5):
            commands::app_settings_get,
            commands::app_settings_mark_tour_completed,
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
        .export(specta_typescript::Typescript::default(), "../src/lib/ipc/bindings.ts")
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
            Ok(())
        })
        .run(tauri::generate_context!())
        // Tauri's run() only returns on a fatal init failure (e.g., missing
        // webview runtime). There's nothing to recover to — crash loudly.
        .expect("error while running tauri application");
}
