mod commands;
pub mod accounts;
pub mod error;
pub mod forge;
pub mod instances;
pub mod jre;
pub mod launch;
pub mod logs;
pub mod network;
pub mod paths;
pub mod versions;

use tauri_specta::{collect_commands, collect_events, Builder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::greet,
            commands::network_activity,
            commands::network_audit_violations,
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
        ])
        .events(collect_events![
            network::DownloadProgress,
            versions::InstallProgress,
            launch::ProcessSpawned,
            launch::ProcessExited,
        ]);

    #[cfg(debug_assertions)]
    builder
        .export(specta_typescript::Typescript::default(), "../src/lib/ipc/bindings.ts")
        // Dev-only: if the bindings file cannot be written, the dev tree is
        // unwritable and there's no graceful path forward — crash loudly.
        .expect("Failed to export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
