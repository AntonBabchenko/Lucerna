//! Tauri-команды фичи «Свой сервер» (План 1: создание/список/удаление).

use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;
use crate::servers_runtime::schema::{ServerFile, ServerWithStatus, UploadConfig};
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

/// Собрать `ServerWithStatus` из файла + живого рантайм-статуса (running/pid/
/// port) + флага наличия пароля в keyring. Единый источник для list/rename/
/// update — чтобы не дублировать логику обогащения статуса.
fn status_of(base: &std::path::Path, file: &ServerFile) -> ServerWithStatus {
    let rp = crate::paths::server_paths(base, &file.id);
    let running = crate::servers_runtime::runtime::is_running(&file.id);
    let pid = crate::servers_runtime::runtime::running_pid(&file.id);
    let port = crate::servers_runtime::runtime::read_port(&rp.runtime);
    let upw = crate::accounts::keychain::retrieve(&crate::accounts::keychain::sftp_password_key(
        &file.id,
    ))
    .ok()
    .flatten()
    .is_some();
    ServerWithStatus::from_file(file, running, pid, port, upw)
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
        handled_log_sig: None,
        upload: None,
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
    Ok(ServerWithStatus::from_file(&file, false, None, None, false))
}

/// Перечислить все серверы в `<app_data>/servers/`. Возвращает живой статус
/// (running / pid / port) из процессного менеджера.
#[tauri::command]
#[specta::specta]
pub fn server_list(app: AppHandle) -> Result<Vec<ServerWithStatus>> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    Ok(store::list_all(&base)?
        .iter()
        .map(|f| status_of(&base, f))
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
/// Возвращает ошибку если сервер запущен — сначала остановите его.
#[tauri::command]
#[specta::specta]
pub fn server_delete(app: AppHandle, id: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    store::delete_server(&base, &id)?;
    let _ = crate::accounts::keychain::delete(&crate::accounts::keychain::sftp_password_key(&id));
    Ok(())
}

/// Переименовать сервер. Имя триммится на бэкенде; фронт гейтит пустое/длину.
#[tauri::command]
#[specta::specta]
pub fn server_rename(app: AppHandle, id: String, name: String) -> Result<ServerWithStatus> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let file = store::rename_server(&base, &id, &name)?;
    Ok(status_of(&base, &file))
}

/// Изменить heap (`max_heap_mb`) и доп. JVM-аргументы сервера. Применяется при
/// следующем старте (баннер «перезапусти» показывает фронт, если запущен).
#[tauri::command]
#[specta::specta]
pub fn server_update_runtime_config(
    app: AppHandle,
    id: String,
    max_heap_mb: u32,
    extra_jvm_args: String,
) -> Result<ServerWithStatus> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let file = store::update_runtime_config(&base, &id, max_heap_mb, &extra_jvm_args)?;
    Ok(status_of(&base, &file))
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

/// Перечислить `.jar` и `.jar.disabled` файлы в папке `mods/` сервера.
/// Возвращает отсортированный список имён файлов. Если папка отсутствует —
/// возвращает пустой список.
#[tauri::command]
#[specta::specta]
pub fn server_list_mods(app: AppHandle, id: String) -> Result<Vec<String>> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&mods) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let low = n.to_ascii_lowercase();
            if low.ends_with(".jar") || low.ends_with(".jar.disabled") {
                out.push(n);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Удалить мод из папки `mods/` сервера по имени файла.
/// Идемпотентно: файл уже удалён → `Ok`.
/// Отклоняет небезопасные имена (path traversal).
#[tauri::command]
#[specta::specta]
pub fn server_delete_mod(app: AppHandle, id: String, filename: String) -> Result<()> {
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(crate::error::Error::io("<mod>", "invalid filename"));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let path = mods.join(&filename);
    if !path.starts_with(&mods) {
        return Err(crate::error::Error::io("<mod>", "path escapes mods dir"));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::Error::io(path.display().to_string(), e)),
    }
}

/// Диагностировать сервер: читает `server-latest.log`, прогоняет паттерны
/// (`diagnose_server_log`, `dist_crash_tokens`, `classify_client_only_mods`),
/// и возвращает полную диагностику вместе с классификацией статуса.
///
/// Если лог отсутствует или пуст — возвращает `ServerDiagnosis` со статусом `None`
/// и пустыми срезами (не ошибку).
#[tauri::command]
#[specta::specta]
pub async fn server_diagnose(
    app: AppHandle,
    id: String,
) -> Result<crate::logs::diagnose::server::ServerDiagnosis> {
    use crate::logs::diagnose::server::{
        classify_client_only_mods, diagnose_server_log, dist_crash_tokens, forge_client_skip_count,
        ServerDiagnosis,
    };

    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let content = crate::logs::read::read_with_cap(&p.logs.join("server-latest.log"), 1024 * 1024)
        .unwrap_or_default();
    if content.is_empty() {
        return Ok(ServerDiagnosis {
            status: crate::logs::diagnose::DiagnosisStatus::None,
            diagnosis: None,
            client_mods: Vec::new(),
            forge_skip_count: None,
            log_signature: None,
        });
    }
    let signature = crate::logs::diagnose::log_signature(&content);
    let diagnosis = diagnose_server_log(&content);

    let mut mods: Vec<(String, crate::mods::local::ModEnvironment)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&p.mods) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().ends_with(".jar") {
                continue;
            }
            let env = std::fs::read(e.path())
                .ok()
                .map(|b| crate::mods::local::read_jar_environment(&b))
                .unwrap_or(crate::mods::local::ModEnvironment::Unknown);
            mods.push((name, env));
        }
    }
    let tokens = dist_crash_tokens(&content);
    let is_client_crash = diagnosis
        .as_ref()
        .map(|d| d.pattern_id == "server-client-only-mod-crash")
        .unwrap_or(false);
    let client_mods = if is_client_crash {
        classify_client_only_mods(&mods, &tokens)
    } else {
        Vec::new()
    };

    let handled = crate::servers_runtime::store::read_server_json(&p.json)
        .ok()
        .and_then(|f| f.handled_log_sig);
    let status = match &diagnosis {
        Some(d) => {
            crate::logs::diagnose::classify_status(d, 0, None, &signature, handled.as_deref())
        }
        None => crate::logs::diagnose::DiagnosisStatus::None,
    };

    Ok(ServerDiagnosis {
        status,
        diagnosis,
        client_mods,
        forge_skip_count: forge_client_skip_count(&content),
        log_signature: Some(signature),
    })
}

/// Удалить список модов из папки `mods/` сервера по именам файлов.
/// Если задан `log_signature` — записывает его в `server.json` как
/// подпись обработанного лога (идемпотентный маркер «диагноз применён»).
///
/// Идемпотентно: уже удалённый файл → `Ok`. Отклоняет небезопасные имена
/// файлов (защита от path traversal).
#[tauri::command]
#[specta::specta]
pub async fn server_remove_mods(
    app: AppHandle,
    id: String,
    filenames: Vec<String>,
    log_signature: Option<String>,
) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    for f in &filenames {
        if !crate::servers_runtime::runtime::is_safe_mod_name(f) {
            return Err(Error::io("<mod>", "invalid filename"));
        }
        let path = p.mods.join(f);
        if !path.starts_with(&p.mods) {
            return Err(Error::io("<mod>", "path escapes mods dir"));
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(path.display().to_string(), e)),
        }
    }
    if let Some(sig) = log_signature {
        if let Ok(mut file) = crate::servers_runtime::store::read_server_json(&p.json) {
            file.handled_log_sig = Some(sig);
            crate::servers_runtime::store::write_server_json(&p.json, &file)?;
        }
    }
    Ok(())
}

/// Сохранить конфигурацию SFTP-загрузки сервера. Если передан `password` —
/// сохраняет его в связке ключей ОС (пароль никогда не записывается в
/// `server.json`). Идемпотентно: повторный вызов перезаписывает конфигурацию
/// и/или пароль.
#[tauri::command]
#[specta::specta]
pub fn server_set_upload_config(
    app: AppHandle,
    id: String,
    config: UploadConfig,
    password: Option<String>,
) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let mut file = crate::servers_runtime::store::read_server_json(&p.json)?;
    file.upload = Some(config);
    crate::servers_runtime::store::write_server_json(&p.json, &file)?;
    if let Some(pw) = password {
        crate::accounts::keychain::store(&crate::accounts::keychain::sftp_password_key(&id), &pw)?;
    }
    Ok(())
}

/// Загрузить серверный `runtime/` на SFTP-хост. Сервер должен быть остановлен.
///
/// При первом подключении или изменении ключа хоста возвращает ошибку
/// `SftpHostKeyMismatch`, если `accept_new_host_key == false`. При `true`
/// доверяет новому ключу и сохраняет его отпечаток в `server.json`.
#[tauri::command]
#[specta::specta]
pub async fn server_upload(app: AppHandle, id: String, accept_new_host_key: bool) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(crate::error::Error::ServerAlreadyRunning { id });
    }
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let cfg = file
        .upload
        .ok_or(crate::error::Error::UploadNotConfigured)?;
    let password =
        crate::accounts::keychain::retrieve(&crate::accounts::keychain::sftp_password_key(&id))?
            .ok_or(crate::error::Error::UploadNotConfigured)?;
    let new_fp = crate::servers_runtime::transfer::upload_server(
        &app,
        &id,
        &cfg,
        &password,
        accept_new_host_key,
    )
    .await?;
    if let Some(fp) = new_fp {
        let mut f2 = crate::servers_runtime::store::read_server_json(&p.json)?;
        if let Some(u) = f2.upload.as_mut() {
            u.known_host_fp = Some(fp);
        }
        crate::servers_runtime::store::write_server_json(&p.json, &f2)?;
    }
    Ok(())
}

/// Экспортировать серверный `runtime/` в ZIP-архив по пути `dest_path`.
/// Исключает `logs/` и `installer.jar` (те же правила, что у SFTP-загрузки).
#[tauri::command]
#[specta::specta]
pub fn server_export_zip(app: AppHandle, id: String, dest_path: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    crate::servers_runtime::transfer::export_zip(&p.runtime, std::path::Path::new(&dest_path))
}

/// Создать клиентский инстанс из сервера: та же версия + лоадер, моды сервера
/// скопированы в инстанс, и опционально сервер прописан в список мультиплеера
/// (`servers.dat`) нового инстанса. Сервер читается только на чтение.
#[tauri::command]
#[specta::specta]
pub async fn server_create_client_instance(
    app: AppHandle,
    server_id: String,
    name: String,
    add_to_multiplayer: bool,
) -> Result<crate::servers_runtime::to_instance::ClientInstanceResult> {
    let cf_key = crate::mods::curseforge::keyring::resolve();
    crate::servers_runtime::to_instance::create_client_instance(
        &app,
        &server_id,
        &name,
        add_to_multiplayer,
        "https://api.modrinth.com",
        "https://api.curseforge.com",
        cf_key.as_deref(),
    )
    .await
}

/// Join info for a server: host LAN addresses + the server's port + online-mode.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerConnectivity {
    pub lan_addresses: Vec<String>,
    pub port: Option<u16>,
    pub online_mode: bool,
}

/// Read the server's connectivity snapshot: host LAN IPv4s, the configured port,
/// and `online-mode` (from `server.properties`; defaults true when unset).
#[tauri::command]
#[specta::specta]
pub fn server_connectivity(app: AppHandle, id: String) -> Result<ServerConnectivity> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    let port = crate::servers_runtime::runtime::read_port(&rt);
    let online_mode = match std::fs::read_to_string(rt.join("server.properties")) {
        Ok(raw) => crate::servers_runtime::properties::ServerProperties::parse(&raw)
            .get("online-mode")
            .map(|v| v != "false")
            .unwrap_or(true),
        Err(_) => true,
    };
    Ok(ServerConnectivity {
        lan_addresses: crate::process::local_ipv4_addresses(),
        port,
        online_mode,
    })
}

/// Открыть папку `runtime/` сервера в системном файловом менеджере.
/// Создаёт папку, если она ещё не существует. Использует тот же
/// механизм, что и `open_saves_folder` (`tauri_plugin_opener`).
#[tauri::command]
#[specta::specta]
pub async fn server_open_folder(app: AppHandle, id: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).runtime;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}
