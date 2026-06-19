//! Tauri-команды фичи «Свой сервер» (План 1: создание/список/удаление).

use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;
use crate::servers_runtime::schema::{ServerFile, ServerWithStatus, UploadConfig};
use crate::servers_runtime::{create, store};
use tauri::AppHandle;

/// Собрать `ServerWithStatus` из файла + живого рантайм-статуса (running/pid/
/// port) + флага наличия пароля в keyring. Единый источник для list/rename/
/// update — чтобы не дублировать логику обогащения статуса.
fn status_of(base: &std::path::Path, file: &ServerFile) -> ServerWithStatus {
    let rp = crate::paths::server_paths(base, &file.id);
    // Reconcile against the persisted PID so a server still alive after a
    // launcher restart is not shown as "Stopped" (Bug A part 2).
    let in_mem = crate::servers_runtime::runtime::running_pid(&file.id);
    let recorded = crate::servers_runtime::pid::read_pid(&rp.pid);
    let alive_ours = recorded
        .map(|pid| {
            crate::platform::process_alive(pid)
                && crate::platform::process_image_matches(pid, "java")
        })
        .unwrap_or(false);
    let (running, pid) =
        crate::servers_runtime::runtime::reconcile_running(in_mem, recorded, alive_ours);
    let port = crate::servers_runtime::runtime::read_port(&rp.runtime);
    let upw = crate::accounts::keychain::retrieve(&crate::accounts::keychain::sftp_password_key(
        &file.id,
    ))
    .ok()
    .flatten()
    .is_some();
    ServerWithStatus::from_file(file, running, pid, port, upw)
}

/// Pre-spawn launch-outcome diagnosis: when the server is stopped and the log
/// produced no actionable diagnosis, classify the first blocking pre-condition
/// (orphan process → busy port → EULA) into a fixable `ServerDiagnosis`.
fn preflight_diagnosis(
    p: &crate::paths::ServerPaths,
    id: &str,
) -> Option<crate::logs::diagnose::server::ServerDiagnosis> {
    use crate::servers_runtime::preflight;
    if crate::servers_runtime::runtime::is_running(id) {
        return None;
    }
    let recorded = crate::servers_runtime::pid::read_pid(&p.pid);
    let port = crate::servers_runtime::runtime::read_port(&p.runtime).unwrap_or(0);
    let eula_ok = crate::servers_runtime::store::read_server_json(&p.json)
        .map(|f| f.eula_accepted)
        .unwrap_or(true);
    let finding = preflight::orphan_finding(recorded)
        .or_else(|| {
            preflight::port_in_use(port).then_some(preflight::PreflightFinding::PortInUse(port))
        })
        .or_else(|| preflight::eula_finding(eula_ok))?;
    Some(crate::logs::diagnose::server::diagnosis_from_preflight(
        finding,
    ))
}

/// Accept the EULA for this server (writes runtime/eula.txt and flips the
/// stored flag) so the next start passes the pre-spawn gate.
#[tauri::command]
#[specta::specta]
pub fn server_accept_eula(app: AppHandle, id: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    crate::servers_runtime::eula::write_eula(&p.runtime.join("eula.txt"), true)?;
    if let Ok(mut f) = crate::servers_runtime::store::read_server_json(&p.json) {
        f.eula_accepted = true;
        crate::servers_runtime::store::write_server_json(&p.json, &f)?;
    }
    Ok(())
}

/// Kill a leftover server process holding this server's world (the PID the
/// diagnoser surfaced as `orphan_pid`), then clear the stale PID file. The UI
/// retries start afterwards.
#[tauri::command]
#[specta::specta]
pub fn server_stop_orphan(app: AppHandle, id: String, pid: u32) -> Result<()> {
    if crate::platform::process_alive(pid) && crate::platform::process_image_matches(pid, "java") {
        crate::platform::kill_process_tree(pid);
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    crate::servers_runtime::pid::clear_pid(&crate::paths::server_paths(&base, &id).pid);
    Ok(())
}

/// Change the server's listen port in `server.properties` (validated 1..=65535).
#[tauri::command]
#[specta::specta]
pub fn server_change_port(app: AppHandle, id: String, port: u16) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let props_path = p.runtime.join("server.properties");
    let raw = std::fs::read_to_string(&props_path).unwrap_or_default();
    let mut props = crate::servers_runtime::properties::ServerProperties::parse(&raw);
    props.set_validated("server-port", &port.to_string())?;
    std::fs::create_dir_all(&p.runtime)
        .map_err(|e| Error::io(p.runtime.display().to_string(), e))?;
    std::fs::write(&props_path, props.serialize()).map_err(|e| Error::io("<server.properties>", e))
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
    create::redownload_server_artifact(&app, &base, &file).await?;
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
        classify_client_only_mods, diagnose_server_log, dist_crash_tokens, extract_missing_dep_ids,
        forge_client_skip_count, pick_diagnosable, server_repair_for, ServerDiagnosis,
        ServerRepairTag,
    };

    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let content = crate::logs::read::read_with_cap(&p.logs.join("server-latest.log"), 1024 * 1024)
        .unwrap_or_default();
    // Phase 2: also consider the freshest crash report (crash-reports/*.txt), not
    // only server-latest.log. The richer of the two is diagnosed.
    let crash_text = newest_crash_text(&p.runtime.join("crash-reports"), 1024 * 1024);
    if content.is_empty() && crash_text.is_none() {
        if let Some(d) = preflight_diagnosis(&p, &id) {
            return Ok(d);
        }
        return Ok(ServerDiagnosis {
            status: crate::logs::diagnose::DiagnosisStatus::None,
            diagnosis: None,
            client_mods: Vec::new(),
            forge_skip_count: None,
            log_signature: None,
            server_repair: None,
            port_in_use: None,
            orphan_pid: None,
            corrupt_jar: None,
            suggested_heap_mb: None,
            conflict_mods: Vec::new(),
        });
    }
    let diag_input = pick_diagnosable(&content, crash_text.as_deref());
    let signature = crate::logs::diagnose::log_signature(diag_input);
    let diagnosis = diagnose_server_log(diag_input);

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
    let tokens = dist_crash_tokens(diag_input);
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
    let mut status = match &diagnosis {
        Some(d) => {
            crate::logs::diagnose::classify_status(d, 0, None, &signature, handled.as_deref())
        }
        None => crate::logs::diagnose::DiagnosisStatus::None,
    };

    if diagnosis.is_none() {
        if let Some(d) = preflight_diagnosis(&p, &id) {
            return Ok(d);
        }
    }

    // Phase 2: attach the one-click repair tag + its fix-params per kind.
    let mut server_repair = diagnosis
        .as_ref()
        .and_then(|d| server_repair_for(&d.pattern_id));
    let mut corrupt_jar = None;
    let mut suggested_heap_mb = None;
    let mut conflict_mods = Vec::new();
    let mut orphan_pid = None;
    let mut server_client_mods = client_mods;
    if let Some(tag) = server_repair {
        let file = crate::servers_runtime::store::read_server_json(&p.json).ok();
        match tag {
            ServerRepairTag::RaiseHeap => {
                let cur = file.as_ref().map(|f| f.max_heap_mb).unwrap_or(0);
                suggested_heap_mb = crate::logs::diagnose::repair::suggest_heap_mb(
                    cur,
                    crate::platform::total_system_ram_mb(),
                );
            }
            ServerRepairTag::LowerHeap => {
                suggested_heap_mb = Some(crate::instances::memory::recommended_max_mb(
                    crate::platform::total_system_ram_mb(),
                ));
            }
            ServerRepairTag::RedownloadServerJar => {
                corrupt_jar = crate::logs::diagnose::repair::extract_corrupt_jar(diag_input);
            }
            ServerRepairTag::DisableMods => {
                // Conflict: cite ids from the log. Mixin: reuse the client-mod
                // checklist seeded by dist_crash_tokens.
                conflict_mods = crate::logs::diagnose::repair::extract_conflict_mods(diag_input);
                if server_client_mods.is_empty() {
                    let toks = dist_crash_tokens(diag_input);
                    server_client_mods = classify_client_only_mods(&mods, &toks);
                }
            }
            ServerRepairTag::InstallMissingDep => {
                conflict_mods = extract_missing_dep_ids(diag_input);
            }
            ServerRepairTag::StopOrphanAndRetry => {
                // The log-detected session lock needs the live orphan PID — the
                // log path doesn't run preflight (where Phase 1 fills it).
                let recorded = crate::servers_runtime::pid::read_pid(&p.pid);
                orphan_pid = match crate::servers_runtime::preflight::orphan_finding(recorded) {
                    Some(crate::servers_runtime::preflight::PreflightFinding::OrphanRunning(
                        pid,
                    )) => Some(pid),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    // A session lock with no live orphan we own has nothing to kill — drop to
    // advisory rather than show a no-op "Stop leftover" button.
    if server_repair == Some(ServerRepairTag::StopOrphanAndRetry) && orphan_pid.is_none() {
        server_repair = None;
    }
    // Server fixes live in `server_repair`, not the client `Diagnosis.repair`, so
    // classify_status returned Advisory. Upgrade to Actionable when a real fix is
    // offered (never override Handled/None).
    if server_repair.is_some() && status == crate::logs::diagnose::DiagnosisStatus::Advisory {
        status = crate::logs::diagnose::DiagnosisStatus::Actionable;
    }

    Ok(ServerDiagnosis {
        status,
        diagnosis,
        client_mods: server_client_mods,
        forge_skip_count: forge_client_skip_count(diag_input),
        log_signature: Some(signature),
        server_repair,
        port_in_use: None,
        orphan_pid,
        corrupt_jar,
        suggested_heap_mb,
        conflict_mods,
    })
}

/// Read the newest `crash-*.txt` (by mtime) under `dir`, capped at `cap` bytes.
/// Returns `None` when the directory is absent or holds no crash report.
fn newest_crash_text(dir: &std::path::Path, cap: u64) -> Option<String> {
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("crash-") || !name.ends_with(".txt") {
            continue;
        }
        let Ok(m) = e.metadata().and_then(|md| md.modified()) else {
            continue;
        };
        if newest.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
            newest = Some((m, e.path()));
        }
    }
    let (_, path) = newest?;
    crate::logs::read::read_with_cap(&path, cap).ok()
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

/// Raise the server's max heap to `to_mb` (the diagnoser's suggested value) and
/// persist. The UI restarts the server afterward.
#[tauri::command]
#[specta::specta]
pub fn server_raise_heap(app: AppHandle, id: String, to_mb: u32) -> Result<()> {
    if to_mb == 0 {
        return Err(Error::io("<heap>", "heap must be > 0"));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    crate::servers_runtime::store::set_max_heap_mb(&p.json, to_mb)
}

/// Lower the server's max heap to `to_mb` (a safe value <= physical RAM) and
/// persist. Used for the heap-too-big fix.
#[tauri::command]
#[specta::specta]
pub fn server_lower_heap(app: AppHandle, id: String, to_mb: u32) -> Result<()> {
    if to_mb == 0 {
        return Err(Error::io("<heap>", "heap must be > 0"));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    crate::servers_runtime::store::set_max_heap_mb(&p.json, to_mb)
}

/// Re-download the server's main jar (corrupt-jar fix). Re-runs the same
/// create-time artifact resolution + download for the stored loader/version.
/// Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_redownload_jar(app: AppHandle, id: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let file = crate::servers_runtime::store::read_server_json(
        &crate::paths::server_paths(&base, &id).json,
    )?;
    crate::servers_runtime::create::redownload_server_artifact(&app, &base, &file).await
}

/// Reinstall the loader (Forge/NeoForge/Fabric/Quilt) for this server by
/// re-running the create-time installer. No-op-safe for Vanilla. Server must be
/// stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_reinstall_loader(app: AppHandle, id: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let file = crate::servers_runtime::store::read_server_json(
        &crate::paths::server_paths(&base, &id).json,
    )?;
    crate::servers_runtime::create::redownload_server_artifact(&app, &base, &file).await
}

/// Disable (rename to `*.disabled`) a list of mods in the server's `mods/`.
/// Reversible alternative to `server_remove_mods` for conflict/mixin fixes.
/// Records `log_signature` as handled when given. Rejects unsafe filenames.
#[tauri::command]
#[specta::specta]
pub async fn server_disable_mods(
    app: AppHandle,
    id: String,
    filenames: Vec<String>,
    log_signature: Option<String>,
) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    for f in &filenames {
        if !crate::servers_runtime::runtime::is_safe_mod_name(f) {
            return Err(Error::io("<mod>", "invalid filename"));
        }
        let src = p.mods.join(f);
        if !src.starts_with(&p.mods) {
            return Err(Error::io("<mod>", "path escapes mods dir"));
        }
        let dst = p.mods.join(format!("{f}.disabled"));
        match std::fs::rename(&src, &dst) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(src.display().to_string(), e)),
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

/// Install missing dependency mods into the server's `mods/` (B9/B10 fix).
/// `mod_ids` come from the diagnosis `conflict_mods`. Resolves each id to a
/// concrete version via the shared dep resolver and downloads through `network::`.
/// Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_install_missing_dep(
    app: AppHandle,
    id: String,
    mod_ids: Vec<String>,
) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    // The mod cache lives under app_dir (same root the instance installer uses).
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let cf_key = crate::mods::curseforge::keyring::resolve();
    crate::mods::dep_resolve::install_missing_into_dir(
        &base,
        &p.mods,
        &mod_ids,
        &file.mc_version,
        file.loader,
        cf_key,
    )
    .await
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
