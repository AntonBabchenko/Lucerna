mod commands;
pub mod accounts;
pub mod error;
pub mod jre;
pub mod launch;
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
            commands::get_account,
            commands::set_offline_account,
            commands::list_versions,
            commands::install_version,
            commands::install_and_launch,
            commands::stop_minecraft,
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
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        // Tauri's run() only returns on a fatal init failure (e.g., missing
        // webview runtime). There's nothing to recover to — crash loudly.
        .expect("error while running tauri application");
}
