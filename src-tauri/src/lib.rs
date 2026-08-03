pub mod accounts;
pub mod cli;
mod commands;
pub mod data_root;
pub mod datapacks;
pub mod deeplink;
pub mod diag;
pub mod error;
pub mod forge;
pub mod instances;
pub mod journal;
pub mod jre;
pub mod l10n;
pub mod launch;
pub mod logs;
pub mod mods;
pub mod naming;
pub mod network;
pub mod paths;
pub mod pathsafe;
pub mod platform;
pub mod playtime;
pub mod process;
pub mod screenshots;
pub mod servers;
pub mod servers_runtime;
pub mod shortcuts;
/// In-process test seams replacing the `LUCERNA_*` env-var test overrides
/// (see the module docs for the glibc `setenv`/`getenv` heap-corruption flake
/// they avoid). Public so the `tests/` integration binaries can call `scope`.
pub mod test_seam;
pub mod tray;
pub mod uninstall_cleanup;
pub mod update;
pub mod verify;
pub mod versions;
pub mod window;
pub mod worlds;

/// General-purpose process-wide lock for the few tests that need exclusive
/// access to a global (e.g. the in-memory keyring backend's `TEST_KEY`).
///
/// Env-var test overrides no longer go through here — they migrated to the
/// in-process [`test_seam`] registry, so nothing mutates `environ` during the
/// parallel test run anymore. This delegates to `test_seam`'s scope lock so
/// that those non-env global-state tests and the seam scopes share ONE
/// serialization domain (a key set under this lock can't be cleared by a
/// concurrent seam-holding test, and vice versa).
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::test_seam::serialization_lock()
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
            commands::get_cosmetics,
            commands::cape_texture,
            commands::set_active_cape,
            commands::clear_active_cape,
            commands::upload_skin,
            commands::reset_skin,
            commands::skin_library_list,
            commands::skin_library_save,
            commands::skin_library_update,
            commands::skin_library_delete,
            commands::list_versions,
            commands::install_version,
            commands::install_instance,
            commands::launch_instance,
            commands::instance_quick_play_support,
            commands::stop_instance,
            commands::list_log_files,
            commands::read_log_file,
            commands::latest_crash,
            commands::diagnose_log,
            commands::diagnose_latest,
            commands::annotate_log_file,
            commands::annotate_log_text,
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
            commands::list_screenshots,
            commands::list_all_screenshots,
            commands::screenshot_thumbnail,
            commands::screenshot_preview,
            commands::delete_screenshot,
            commands::save_screenshot_copy,
            commands::reveal_screenshot,
            commands::open_screenshots_folder,
            commands::clipboard_read_text,
            commands::copy_screenshot_to_clipboard,
            commands::save_annotated_screenshot,
            commands::annotated_default_path,
            commands::list_saved_servers,
            commands::add_saved_server,
            commands::remove_saved_server,
            commands::ping_server,
            commands::open_log_folder,
            commands::delete_log_file,
            commands::clear_old_logs,
            commands::apply_log_retention,
            commands::instance_journal_read,
            commands::instance_journal_clear,
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
            commands::set_instance_min_heap,
            commands::set_instance_jvm_args,
            commands::instance_memory_bounds,
            commands::detach_instance_pack,
            commands::open_instance_folder,
            commands::verify_instance,
            commands::repair_instance,
            commands::set_instance_icon,
            commands::clear_instance_icon,
            commands::instance_icon,
            commands::pre_launch_check,
            commands::running_instances,
            commands::get_playtime,
            commands::window_set_compact,
            commands::window_set_expanded_floor,
            // Mod browser (v0.5.0 sub-feature 3):
            commands::mods_search,
            commands::mods_project,
            commands::mods_projects,
            commands::mods_versions,
            commands::mods_changelog,
            commands::mods_plugin_versions,
            commands::mods_filter_satisfying,
            commands::mods_resolve_deps,
            commands::optimise_resolve,
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
            commands::asset_update_one,
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
            // Instance clone (Session 3):
            commands::clone_instance,
            commands::clone_instance_scan,
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
            commands::server_kill,
            commands::server_restart,
            commands::server_send_command,
            // Own server (Plan 3: properties read/write):
            commands::server_read_properties,
            commands::server_write_properties,
            // Own server (Plan 3: mod management + folder open):
            commands::server_list_mods,
            commands::server_list_mods_enriched,
            commands::server_enrich_mods,
            commands::server_enrich_plugins,
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
            commands::server_upload_resume_state,
            commands::server_upload_preflight,
            commands::server_cancel_upload,
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
            // Own server (S2: mod content management — browse-install, enable,
            // local install, datapacks):
            commands::server_install_mod,
            commands::server_check_mod_updates,
            commands::server_update_one,
            commands::server_enable_mod,
            commands::server_disable_mod,
            commands::server_install_local,
            commands::server_list_datapacks,
            commands::server_install_datapack,
            commands::server_remove_datapack,
            // Own server (plugin management: Paper/Purpur runtime/plugins/):
            commands::server_list_plugins,
            commands::server_list_plugins_enriched,
            commands::server_install_plugin,
            commands::server_check_plugin_updates,
            commands::server_update_plugin_one,
            commands::server_install_plugin_local,
            commands::server_enable_plugin,
            commands::server_disable_plugin,
            commands::server_delete_plugin,
            commands::server_open_plugins_folder,
            commands::server_open_mods_folder,
            // Own server (core switch: Vanilla->Paper/Purpur, Paper<->Purpur):
            commands::server_switch_core,
            commands::server_core_versions,
            // Data-root location:
            commands::get_data_location,
            commands::data_root_size_bytes,
            commands::set_data_location,
            commands::plan_data_location_change,
            commands::adopt_data_location,
            // Desktop integration (inbound intents, lucerna:// scheme, shortcuts):
            commands::take_pending_intent,
            commands::modpack_resolve_url,
            commands::url_scheme_key,
            commands::url_scheme_state,
            commands::url_scheme_register,
            commands::url_scheme_unregister,
            commands::shortcut_supported,
            commands::shortcut_create,
            commands::shortcut_default_name,
            // Client-side datapacks (Batch G: IPC surface):
            commands::datapacks_list_library,
            commands::datapacks_install_from_file,
            commands::datapacks_remove_from_library,
            commands::datapacks_list_for_world,
            commands::datapacks_add_to_world,
            commands::datapacks_remove_from_world,
            commands::datapacks_set_enabled_in_world,
            // In-game mod localization (translation coverage + override editor):
            commands::l10n_coverage,
            commands::l10n_namespace_keys,
            commands::l10n_set_override,
            commands::l10n_apply,
            // In-game mod localization (AI pre-fill):
            commands::l10n_prefill_estimate,
            commands::l10n_prefill_start,
            commands::l10n_prefill_cancel,
            commands::l10n_prefill_set_key,
            commands::l10n_prefill_key_status,
            commands::l10n_prefill_test_key,
            commands::l10n_revert_machine,
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
            commands::DataMigrationProgress,
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
    let context = tauri::generate_context!();

    // Headless uninstall cleanup: `lucerna.exe --uninstall-cleanup [--list]`,
    // invoked by the NSIS uninstaller hook (installer/hooks.nsh) while the
    // binary still exists on disk. Must run before any Tauri/webview/plugin
    // init — no window, no single-instance guard, no data-root migration.
    #[cfg(windows)]
    if let Some(code) = uninstall_cleanup::maybe_run_from_args(&context.config().identifier) {
        std::process::exit(code);
    }

    let builder = specta_builder();

    // Dev-only no-app bindings regen: `cargo run -- --export-bindings [--check]`
    // exports the bindings and exits before the app launches.
    #[cfg(debug_assertions)]
    if let Some(code) = maybe_export_bindings_from_args(&builder) {
        std::process::exit(code);
    }

    // Cold start: a `lucerna://` URL dispatched by the OS, or a desktop
    // shortcut's `--launch`, arrives as argv of THIS process. Park it in the
    // PendingIntent slot below; the frontend drains it on mount
    // (`commands::take_pending_intent`). Parsed AFTER the two headless flags
    // above so neither can be mistaken for an intent.
    let startup_intent = cli::parse(std::env::args());

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
        // a Minecraft session).
        //
        // The second launch's argv is then forwarded: a desktop shortcut's
        // `--launch` or an OS-dispatched `lucerna://` URL lands in the SAME
        // PendingIntent slot a cold start uses, and `intent-pending` nudges the
        // frontend to drain it. Trust model: argv may launch, a URL may only
        // open the import confirmation UI (see `cli`).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::{Emitter, Manager};
            if let Err(e) = crate::tray::restore_from_tray(app) {
                crate::diag!("[single-instance] failed to restore window: {e}");
            }
            if let Some(intent) = crate::cli::parse(argv) {
                app.state::<crate::cli::PendingIntent>().set(intent);
                if let Err(e) = app.emit("intent-pending", ()) {
                    crate::diag!("[single-instance] failed to emit intent-pending: {e}");
                }
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(builder.invoke_handler())
        .manage(window::WindowSizeState::default())
        .manage({
            let slot = cli::PendingIntent::default();
            if let Some(intent) = startup_intent {
                slot.set(intent);
            }
            slot
        })
        .setup(move |app| {
            // Resolve the effective data root before anything else touches app_dir.
            let default_root = crate::paths::default_app_data_dir(app.handle())
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let redirect = crate::paths::redirect_file(app.handle())
                .ok()
                .and_then(|f| crate::data_root::redirect::read(&f).ok().flatten());
            // Portable candidate (`<exe dir>\LucernaData`): WINDOWS release
            // builds only, and only when no explicit redirect exists. Dev must
            // never adopt `target/debug/LucernaData`; the install dir must not
            // be write-probed when the user's explicit choice wins anyway; and
            // macOS is excluded deliberately — a `.app` bundle dir is often
            // writable, but the only macOS update path is Finder's
            // drag-replace, which DELETES the old bundle wholesale: data
            // created inside it would be lost on every update. (Linux is
            // naturally safe — AppImage mounts read-only, deb/rpm install to
            // root-owned paths — but stays excluded until portable mode is
            // designed for it.)
            let portable = if !cfg!(all(windows, not(debug_assertions))) || redirect.is_some() {
                None
            } else {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
                    .map(|exe_dir| {
                        let path = exe_dir.join("LucernaData");
                        let state = if !path.is_dir() {
                            crate::data_root::PortableState::Absent {
                                // Probe writability only when creation could
                                // actually happen.
                                creatable: crate::data_root::migrate::is_available(&exe_dir),
                            }
                        } else if crate::data_root::looks_like_data_root(&path) {
                            crate::data_root::PortableState::Root
                        } else if crate::data_root::migrate::target_is_empty(&path) {
                            crate::data_root::PortableState::EmptyDir
                        } else {
                            crate::data_root::PortableState::Foreign
                        };
                        crate::data_root::PortableCandidate { path, state }
                    })
            };
            let default_has_data =
                default_root.join("app.json").is_file() || default_root.join("instances").is_dir();
            let mut resolved = crate::data_root::resolve_root(
                default_root.clone(),
                default_has_data,
                portable,
                redirect,
                |p| crate::data_root::migrate::is_available(p),
            );
            if resolved.must_create {
                if let Err(e) = std::fs::create_dir_all(&resolved.root) {
                    crate::diag!(
                        "[setup] cannot create portable data root {}: {e} — using OS default",
                        resolved.root.display()
                    );
                    resolved = crate::data_root::Resolved {
                        root: default_root.clone(),
                        configured: None,
                        fell_back: false,
                        must_create: false,
                    };
                }
            }
            // Human-readable trail of WHICH branch picked the root — the
            // launcher log is the only evidence when a user reports "my
            // instances disappeared after an update". Emitted after
            // diag::init below so it lands in lucerna.log.
            let resolution_note = format!(
                "[data-root] root={} ({})",
                resolved.root.display(),
                if resolved.fell_back {
                    "configured root unavailable, temporary default"
                } else if resolved.configured.is_some() {
                    "configured redirect"
                } else if resolved.must_create {
                    "fresh portable root next to the exe"
                } else if resolved.root != default_root {
                    "adopted portable root next to the exe"
                } else {
                    "OS default"
                }
            );
            {
                use tauri::Manager;
                app.manage(crate::data_root::DataRoot(resolved));
            }

            // Open the launcher's own diagnostic log (lucerna.log) first, so
            // subsequent `diag!` lines in setup are captured. Best-effort.
            diag::init(app.handle());
            crate::diag!("{resolution_note}");

            // The main window is created here rather than in tauri.conf.json
            // (the config's `windows` array is empty) so that in release
            // builds its WebView2 profile can live under the resolved data
            // root — keeping browser cache out of %LOCALAPPDATA% and inside
            // the install/data folder. Dev builds keep the default profile
            // location (a dev webview must not mix with a real install's).
            {
                #[allow(unused_mut)]
                let mut main_window =
                    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                        .title("Lucerna")
                        .inner_size(820.0, 520.0)
                        .min_inner_size(820.0, 520.0);
                #[cfg(all(windows, not(debug_assertions)))]
                {
                    use tauri::Manager;
                    main_window = main_window.data_directory(
                        app.state::<crate::data_root::DataRoot>()
                            .0
                            .root
                            .join("webview"),
                    );
                }
                main_window.build()?;
            }

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

            // Self-heal the `lucerna://` registration ONLY if the user opted in
            // and the recorded command points at a different exe (moved install,
            // update, portable copy) — otherwise links would open a binary that
            // is no longer there. With the setting off we touch nothing at all:
            // not the key, not a cleanup, no registry write of any kind.
            if let Ok(settings) = crate::paths::app_file(app.handle())
                .map_err(|e| crate::error::Error::io("<app_file>", e))
                .and_then(|p| crate::instances::store::read_app_json(&p))
            {
                if settings.general.register_url_scheme {
                    if let Ok(exe) = std::env::current_exe() {
                        if crate::platform::protocol::state(&exe)
                            == crate::platform::protocol::SchemeState::RegisteredToOtherPath
                        {
                            match crate::platform::protocol::register(&exe) {
                                Ok(()) => crate::diag!(
                                    "[setup] re-pointed lucerna:// registration at {}",
                                    exe.display()
                                ),
                                Err(e) => crate::diag!(
                                    "[setup] failed to re-point lucerna:// registration: {e}"
                                ),
                            }
                        }
                    }
                }
            }

            // Re-arm per-server auto-backup schedulers. The interval task only
            // lives for the launcher session it was set in, so without this an
            // enabled backup policy silently stops producing snapshots after a
            // restart until the user re-saves settings. Best-effort; spawns its
            // own tasks internally.
            crate::commands::rearm_backup_schedulers(app.handle());

            // Idle refresh task: scan accounts and refresh any Microsoft account
            // whose access token is within 5 minutes of expiry. Mirrors Mojang
            // reference launcher's silent-renew behaviour. Failures are logged to
            // stderr but don't surface to the UI — the next interactive sign-in
            // will prompt the user if the refresh chain is unrecoverable.
            //
            // The scan runs immediately on startup and then every 60s (sleep is at
            // the END of the loop). A launcher that was closed overnight would
            // otherwise spend the first 60s serving an expired token to `Play`,
            // making multiplayer joins fail with "Invalid session" until the first
            // pass eventually fired. `launch_instance` also refreshes just-in-time
            // before building argv, but a proactive startup pass keeps every other
            // token-consuming path (skins, profile) fresh too.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    match crate::accounts::list_accounts(&app_handle) {
                        Ok(accounts) => {
                            for a in accounts {
                                if a.kind != crate::accounts::store::AccountKind::Microsoft {
                                    continue;
                                }
                                let Some(exp) = a.expires_at else {
                                    continue;
                                };
                                if exp <= now + 300.0 {
                                    if let Err(e) =
                                        crate::accounts::microsoft::refresh(&app_handle, &a.id)
                                            .await
                                    {
                                        crate::diag!(
                                            "microsoft refresh: failed for account {}: {e}",
                                            a.id
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            crate::diag!("microsoft refresh: list_accounts failed: {e}");
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            });

            Ok(())
        })
        .build(context)
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Bug A root fix: never orphan server children. On launcher exit,
            // synchronously force-kill every tracked server process, then sweep
            // persisted PID files so a server adopted after a restart (alive on
            // disk but not in this session's map) is killed too, not left to
            // hold its world lock.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                crate::servers_runtime::runtime::kill_all_running();
                // Same for client Minecraft processes: never orphan a running
                // game when the launcher exits.
                crate::launch::spawn::kill_all_running();
                if let Ok(dir) = crate::paths::servers_dir(app_handle) {
                    crate::servers_runtime::runtime::kill_persisted_orphans(&dir);
                }
            }
        });
}
