//! Tauri-команды фичи «Свой сервер» (План 1: создание/список/удаление).

use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;
use crate::servers_runtime::schema::{ServerFile, ServerWithStatus};
use crate::servers_runtime::{create, store};
use tauri::AppHandle;

fn require_loader_version(file: &ServerFile, loader: &str) -> Result<String> {
    file.loader_version
        .clone()
        .ok_or_else(|| Error::ServerJarUnavailable {
            loader: loader.to_string(),
            mc_version: file.mc_version.clone(),
            reason: "loader_version required".into(),
        })
}

/// Создать сервер: разрешить артефакт по лоадеру, скачать/установить,
/// записать `server.json` + `eula.txt`.
#[tauri::command]
#[specta::specta]
pub async fn server_create(
    app: AppHandle,
    name: String,
    mc_version: String,
    loader: LoaderKind,
    loader_version: Option<String>,
    max_heap_mb: u32,
    eula_accepted: bool,
    created_from_instance: Option<String>,
) -> Result<ServerWithStatus> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let id = format!("srv-{}", crate::instances::ids::new_id());
    let file = ServerFile {
        id,
        name,
        mc_version,
        loader,
        loader_version,
        max_heap_mb,
        extra_jvm_args: String::new(),
        created_unix_ms: chrono::Utc::now().timestamp_millis() as f64,
        eula_accepted,
        created_from_instance,
    };
    match file.loader {
        LoaderKind::Vanilla => {
            let (jar_url, sha1) = create::resolve_vanilla_jar(&file.mc_version).await?;
            create::create_vanilla_server(&base, &file, &jar_url, &sha1).await?;
        }
        LoaderKind::Fabric => {
            let installer = create::latest_fabric_installer(&file.mc_version).await?;
            let lv = require_loader_version(&file, "fabric")?;
            let url = crate::servers_runtime::jar::fabric_server_jar_url(
                &file.mc_version,
                &lv,
                &installer,
            );
            create::create_fabric_server(&base, &file, &url).await?;
        }
        LoaderKind::Quilt => {
            let installer = create::latest_quilt_installer(&file.mc_version).await?;
            let lv = require_loader_version(&file, "quilt")?;
            let url = crate::servers_runtime::jar::quilt_server_jar_url(
                &file.mc_version,
                &lv,
                &installer,
            );
            create::create_quilt_server(&base, &file, &url).await?;
        }
        LoaderKind::Forge | LoaderKind::NeoForge => {
            let lv = require_loader_version(&file, "forge/neoforge")?;
            let (url, label) = if matches!(file.loader, LoaderKind::Forge) {
                (
                    crate::servers_runtime::jar::forge_installer_url(&file.mc_version, &lv),
                    "forge",
                )
            } else {
                (
                    crate::servers_runtime::jar::neoforge_installer_url(&lv),
                    "neoforge",
                )
            };
            let component = create::resolve_server_java_component(&file.mc_version).await?;
            crate::jre::ensure_jre(&component, &app, |_, _, _| {}).await?;
            let java_bin = crate::jre::java_executable_path(&component, &app)?;
            create::create_installer_server(&base, &file, &url, &java_bin, label).await?;
        }
    }
    if let Some(inst_id) = &file.created_from_instance {
        let src = crate::paths::mods_dir(&app, inst_id)
            .map_err(|e| crate::error::Error::io("<instance_mods_dir>", e))?;
        let dest = crate::paths::server_paths(&base, &file.id).mods;
        let copied = crate::servers_runtime::create::copy_instance_mods(&src, &dest)?;
        eprintln!("servers: copied {copied} mods from instance {inst_id}");
    }
    Ok(ServerWithStatus::from_file(&file, false, None, None))
}

/// Перечислить все серверы в `<app_data>/servers/`. Возвращает живой статус
/// (running / pid / port) из процессного менеджера.
#[tauri::command]
#[specta::specta]
pub fn server_list(app: AppHandle) -> Result<Vec<ServerWithStatus>> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    Ok(store::list_all(&base)?
        .iter()
        .map(|f| {
            let rp = crate::paths::server_paths(&base, &f.id);
            let running = crate::servers_runtime::runtime::is_running(&f.id);
            let pid = crate::servers_runtime::runtime::running_pid(&f.id);
            let port = crate::servers_runtime::runtime::read_port(&rp.runtime);
            ServerWithStatus::from_file(f, running, pid, port)
        })
        .collect())
}

/// Запустить сервер. Возвращает PID запущенного процесса.
#[tauri::command]
#[specta::specta]
pub async fn server_start(app: AppHandle, id: String) -> Result<u32> {
    crate::servers_runtime::runtime::start(&app, &id).await
}

/// Остановить сервер (graceful stop, затем принудительное завершение при необходимости).
#[tauri::command]
#[specta::specta]
pub async fn server_stop(id: String) -> Result<()> {
    crate::servers_runtime::runtime::stop(&id).await
}

/// Перезапустить сервер (stop если запущен, затем start).
#[tauri::command]
#[specta::specta]
pub async fn server_restart(app: AppHandle, id: String) -> Result<u32> {
    crate::servers_runtime::runtime::restart(&app, &id).await
}

/// Отправить консольную команду на stdin работающего сервера.
#[tauri::command]
#[specta::specta]
pub async fn server_send_command(id: String, line: String) -> Result<()> {
    crate::servers_runtime::runtime::send_command(&id, &line).await
}

/// Удалить сервер и все его данные. Идемпотентно (уже удалён → Ok).
#[tauri::command]
#[specta::specta]
pub fn server_delete(app: AppHandle, id: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    store::delete_server(&base, &id)
}

/// Прочитать `server.properties` сервера как сырой текст. Возвращает пустую
/// строку если файл ещё не создан (первый запуск сервера).
#[tauri::command]
#[specta::specta]
pub fn server_read_properties(app: AppHandle, id: String) -> Result<String> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let path = crate::paths::server_paths(&base, &id)
        .runtime
        .join("server.properties");
    Ok(std::fs::read_to_string(&path).unwrap_or_default())
}

/// Записать `server.properties` сервера. Входной текст парсится и валидируется
/// (только курируемые ключи); неизвестные ключи проходят без проверки.
#[tauri::command]
#[specta::specta]
pub fn server_write_properties(app: AppHandle, id: String, raw: String) -> Result<()> {
    if raw.len() > 64 * 1024 {
        return Err(crate::error::Error::ServerInvalidProperty {
            key: "<file>".into(),
            value: "<raw>".into(),
            reason: "too large".into(),
        });
    }
    let props = crate::servers_runtime::properties::ServerProperties::parse(&raw);
    props.validate()?;
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    std::fs::create_dir_all(&p.runtime)
        .map_err(|e| crate::error::Error::io(p.runtime.display().to_string(), e))?;
    std::fs::write(p.runtime.join("server.properties"), props.serialize())
        .map_err(|e| crate::error::Error::io("<server.properties>", e))
}
