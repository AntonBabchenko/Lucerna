//! Tauri-команды фичи «Свой сервер» (План 1: создание/список/удаление).

use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;
use crate::mods::platform::ServerSideSupport;
use crate::servers_runtime::schema::{ServerFile, ServerWithStatus, UploadConfig};
use crate::servers_runtime::{backup, create, import, store};
use std::collections::HashMap;
use tauri::AppHandle;

/// Result of `server_create`: the new server plus the client-only mods that were
/// automatically set aside (`*.disabled`) so a modpack server can start. The
/// create wizard shows a summary from `quarantined`.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerCreated {
    pub server: ServerWithStatus,
    /// Disabled filenames (`<name>.jar.disabled`) of auto-quarantined client mods.
    pub quarantined: Vec<String>,
}

/// Result of `server_quarantine_client_mods` on an existing server.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct QuarantineReport {
    /// Disabled filenames (`<name>.jar.disabled`) set aside this run.
    pub disabled: Vec<String>,
    /// Client-flagged mods that were kept because another kept mod requires them.
    pub kept_because_required: Vec<String>,
}

/// Build `filename -> server_side` for an *instance's* mods by reading its
/// installed-mods registry (filename → Modrinth project id) and bulk-querying
/// `server_side`. Best-effort: any failure yields a partial/empty map, and the
/// classifier then falls back to each jar's offline `environment`.
async fn server_side_by_instance_mods(
    app: &AppHandle,
    inst_id: &str,
) -> HashMap<String, ServerSideSupport> {
    let mut out: HashMap<String, ServerSideSupport> = HashMap::new();
    let Ok(inst_root) = crate::paths::instance_dir(app, inst_id) else {
        return out;
    };
    let Ok(mods) = crate::mods::installed::list(&inst_root).await else {
        return out;
    };
    let mut pid_by_file: HashMap<String, String> = HashMap::new();
    for m in &mods {
        if m.source == Some(crate::mods::platform::ModSource::Modrinth) {
            if let Some(pid) = &m.project_id {
                pid_by_file.insert(m.filename.clone(), pid.clone());
            }
        }
    }
    if pid_by_file.is_empty() {
        return out;
    }
    let mut ids: Vec<String> = pid_by_file.values().cloned().collect();
    ids.sort();
    ids.dedup();
    let ids_ref: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mr = crate::mods::modrinth::ModrinthClient::new();
    let side_by_pid = mr.server_side_bulk(&ids_ref).await.unwrap_or_default();
    for (file, pid) in pid_by_file {
        if let Some(s) = side_by_pid.get(&pid) {
            out.insert(file, *s);
        }
    }
    out
}

/// Build `filename -> server_side` for an existing *server's* `mods/` by hashing
/// each enabled jar, resolving hashes to Modrinth projects, then bulk-querying
/// `server_side`. Best-effort: failures yield a partial/empty map.
async fn server_side_by_server_mods(
    mods_dir: &std::path::Path,
) -> HashMap<String, ServerSideSupport> {
    use sha1::{Digest, Sha1};
    let mut out: HashMap<String, ServerSideSupport> = HashMap::new();
    let mut sha_by_file: HashMap<String, String> = HashMap::new();
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.to_ascii_lowercase().ends_with(".jar") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(e.path()) {
            sha_by_file.insert(name, hex::encode(Sha1::digest(&bytes)));
        }
    }
    if sha_by_file.is_empty() {
        return out;
    }
    let mut shas: Vec<String> = sha_by_file.values().cloned().collect();
    shas.sort();
    shas.dedup();
    let shas_ref: Vec<&str> = shas.iter().map(String::as_str).collect();
    let mr = crate::mods::modrinth::ModrinthClient::new();
    let pid_by_sha = mr.project_ids_by_hash(&shas_ref).await.unwrap_or_default();
    if pid_by_sha.is_empty() {
        return out;
    }
    let mut pids: Vec<String> = pid_by_sha.values().cloned().collect();
    pids.sort();
    pids.dedup();
    let pids_ref: Vec<&str> = pids.iter().map(String::as_str).collect();
    let side_by_pid = mr.server_side_bulk(&pids_ref).await.unwrap_or_default();
    for (file, sha) in sha_by_file {
        if let Some(pid) = pid_by_sha.get(&sha) {
            if let Some(s) = side_by_pid.get(pid) {
                out.insert(file, *s);
            }
        }
    }
    out
}

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
    let last_exit_code = crate::servers_runtime::exit_state::read(&rp.runtime);
    // Cheap badge signal: a stopped server's latest log is classified here (no
    // jar reads, no network); a running server has no pending diagnosis.
    let diagnosis_status = if running {
        crate::logs::diagnose::DiagnosisStatus::None
    } else {
        // Classify the SAME diagnosable input the diagnosis + handled_log_sig use
        // (latest log + freshest crash report, 1 MB cap, pick_diagnosable) so the
        // handled-fix suppression signature matches and the badge clears after a
        // fix. (One bounded per-server read; server_list is infrequent.)
        let content =
            crate::logs::read::read_with_cap(&rp.logs.join("server-latest.log"), 1024 * 1024)
                .unwrap_or_default();
        let crash = newest_crash_text(&rp.runtime.join("crash-reports"), 1024 * 1024);
        let input = crate::logs::diagnose::server::pick_diagnosable(&content, crash.as_deref());
        crate::logs::diagnose::server::classify_server_status(
            input,
            file.handled_log_sig.as_deref(),
            alive_ours,
        )
    };
    ServerWithStatus::from_file(
        file,
        running,
        pid,
        port,
        upw,
        last_exit_code,
        diagnosis_status,
    )
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
        .or_else(|| preflight::eula_finding(eula_ok))
        .or_else(|| {
            // Advisory; lowest priority — only reached when no actionable
            // orphan/port/EULA finding fired.
            preflight::low_disk(&p.runtime).then_some(preflight::PreflightFinding::LowDisk)
        })?;
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
) -> Result<ServerCreated> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    // Trim + reject empty/duplicate names at the boundary (the wizard also gates
    // this, but two concurrent creates could still collide on the same name).
    let name = store::validate_name(&name, &store::list_all(&base)?, None)?;
    let id = format!("srv-{}", crate::instances::ids::new_id());
    let mut file = ServerFile {
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
        java_component: None,
        upload: None,
    };
    // Cache the MC Java component now (create is online — it downloads the jar)
    // so repeat launches start offline. Best-effort: a miss leaves None and
    // start() resolves + persists it on the next online launch.
    file.java_component =
        crate::servers_runtime::create::resolve_server_java_component(&file.mc_version)
            .await
            .ok();
    provision_loader(&app, &base, &file).await?;
    let mut quarantined: Vec<String> = Vec::new();
    if let Some(inst_id) = &file.created_from_instance {
        let src = crate::paths::mods_dir(&app, inst_id)
            .map_err(|e| crate::error::Error::io("<instance_mods_dir>", e))?;
        let dest = crate::paths::server_paths(&base, &file.id).mods;
        let copied = crate::servers_runtime::create::copy_instance_mods(&src, &dest)?;
        eprintln!("servers: copied {copied} mods from instance {inst_id}");
        // Proactively set aside client-only mods so a modpack server can start
        // instead of crashing one client mod at a time. Best-effort — never
        // fails creation; a metadata miss degrades to offline detection.
        let side_map = server_side_by_instance_mods(&app, inst_id).await;
        match crate::servers_runtime::quarantine::quarantine_with_metadata(&dest, &side_map) {
            Ok((disabled, _)) => {
                if !disabled.is_empty() {
                    eprintln!("servers: quarantined {} client mods", disabled.len());
                }
                quarantined = disabled;
            }
            Err(e) => eprintln!("servers: client-mod quarantine skipped: {e}"),
        }
    }
    Ok(ServerCreated {
        server: ServerWithStatus::from_file(
            &file,
            false,
            None,
            None,
            false,
            None,
            crate::logs::diagnose::DiagnosisStatus::None,
        ),
        quarantined,
    })
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
pub async fn server_stop(app: AppHandle, id: String) -> Result<()> {
    crate::servers_runtime::runtime::stop(&app, &id).await
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

/// Best-effort removal of the Windows firewall allow-rule for this server's port
/// (only when one is present), so deleting a server doesn't leave a stale
/// open-port rule. No-op on non-Windows / when no port or rule exists; gating on
/// presence means UAC is only prompted when there is actually a rule to remove.
fn remove_firewall_rule_if_present(runtime: &std::path::Path) {
    if let Some(port) = crate::servers_runtime::runtime::read_port(runtime) {
        let name = crate::servers_runtime::firewall::rule_name(port);
        if crate::process::firewall_rule_present(&name) {
            let _ = crate::process::firewall_remove_rule_elevated(&name);
        }
    }
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
    let p = crate::paths::server_paths(&base, &id);
    // A server adopted from before a restart isn't in the in-memory map, so the
    // is_running guard above passes; kill any leftover JVM still holding this
    // server's world before removing the directory, or the delete would fail
    // (or re-orphan the process).
    crate::servers_runtime::runtime::kill_owned_pid(&p.pid);
    // Remove the firewall allow-rule we may have added for this server's port so
    // it doesn't linger after the server is gone.
    remove_firewall_rule_if_present(&p.runtime);
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
    // A missing-dep diagnosis whose dep ids we couldn't extract has nothing to
    // install — drop to advisory rather than show a button that would no-op.
    if server_repair == Some(ServerRepairTag::InstallMissingDep) && conflict_mods.is_empty() {
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
    // Dependency safety: refuse to strip a mod another *remaining* mod requires.
    if let Some((filename, required_by)) =
        crate::servers_runtime::quarantine::first_required_conflict(&p.mods, &filenames)
    {
        return Err(Error::ServerModRequiredByOther {
            filename,
            required_by,
        });
    }
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

/// Mark the current latest-log signature as handled for this server, so the
/// diagnosis (and the sidebar "needs a fix" badge) doesn't re-fire after a fix
/// that didn't already record it. Mirrors what remove/disable-mods persist via
/// their `log_signature` param, but derived from disk so the heap/redownload/dep
/// fixes need no extra FE plumbing (no binding change). Best-effort.
fn mark_current_log_handled(p: &crate::paths::ServerPaths) {
    let content = crate::logs::read::read_with_cap(&p.logs.join("server-latest.log"), 1024 * 1024)
        .unwrap_or_default();
    let crash = newest_crash_text(&p.runtime.join("crash-reports"), 1024 * 1024);
    let input = crate::logs::diagnose::server::pick_diagnosable(&content, crash.as_deref());
    if input.is_empty() {
        return;
    }
    let sig = crate::logs::diagnose::log_signature(input);
    if let Ok(mut file) = crate::servers_runtime::store::read_server_json(&p.json) {
        file.handled_log_sig = Some(sig);
        let _ = crate::servers_runtime::store::write_server_json(&p.json, &file);
    }
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
    crate::servers_runtime::store::set_max_heap_mb(&p.json, to_mb)?;
    mark_current_log_handled(&p);
    Ok(())
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
    crate::servers_runtime::store::set_max_heap_mb(&p.json, to_mb)?;
    mark_current_log_handled(&p);
    Ok(())
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
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    provision_loader(&app, &base, &file).await?;
    mark_current_log_handled(&p);
    Ok(())
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
    // Dependency safety: refuse to disable a mod another *remaining* mod requires.
    if let Some((filename, required_by)) =
        crate::servers_runtime::quarantine::first_required_conflict(&p.mods, &filenames)
    {
        return Err(Error::ServerModRequiredByOther {
            filename,
            required_by,
        });
    }
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
) -> Result<crate::mods::dep_resolve::InstallMissingReport> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    // The mod cache lives under app_dir (same root the instance installer uses).
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let cf_key = crate::mods::curseforge::keyring::resolve();
    let report = crate::mods::dep_resolve::install_missing_into_dir(
        &base,
        &p.mods,
        &mod_ids,
        &file.mc_version,
        file.loader,
        cf_key,
    )
    .await?;
    mark_current_log_handled(&p);
    Ok(report)
}

/// Set aside client-only mods on an existing server (rename to `*.disabled`,
/// reversible). Uses platform `server_side` metadata (hash-resolved) plus the
/// offline Fabric/Quilt `environment`, and never sets aside a mod another kept
/// mod requires (dependency-safe). Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_quarantine_client_mods(app: AppHandle, id: String) -> Result<QuarantineReport> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let side_map = server_side_by_server_mods(&p.mods).await;
    let (disabled, result) =
        crate::servers_runtime::quarantine::quarantine_with_metadata(&p.mods, &side_map)?;
    Ok(QuarantineReport {
        disabled,
        kept_because_required: result.kept_because_required,
    })
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
    let (port, online_mode) = match std::fs::read_to_string(rt.join("server.properties")) {
        Ok(raw) => {
            let props = crate::servers_runtime::properties::ServerProperties::parse(&raw);
            let port = props.get("server-port").and_then(|v| v.parse().ok());
            let online_mode = props
                .get("online-mode")
                .map(|v| v != "false")
                .unwrap_or(true);
            (port, online_mode)
        }
        Err(_) => (None, true),
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

/// Разрешить, скачать и установить загрузчик в `runtime/` для данного `ServerFile`.
/// Выделено из `server_create` (DRY): используется и при создании, и при
/// репровижне в ходе импорта.
async fn provision_loader(
    app: &AppHandle,
    base: &std::path::Path,
    file: &ServerFile,
) -> Result<()> {
    match file.loader {
        LoaderKind::Vanilla => {
            let (jar_url, sha1) = create::resolve_vanilla_jar(&file.mc_version).await?;
            create::create_vanilla_server(base, file, &jar_url, &sha1).await?;
        }
        LoaderKind::Fabric => {
            let installer = create::latest_fabric_installer(&file.mc_version).await?;
            let lv = create::require_loader_version(file, "fabric")?;
            let url = crate::servers_runtime::jar::fabric_server_jar_url(
                &file.mc_version,
                &lv,
                &installer,
            );
            create::create_fabric_server(base, file, &url).await?;
        }
        LoaderKind::Quilt => {
            let installer = create::latest_quilt_installer(&file.mc_version).await?;
            let lv = create::require_loader_version(file, "quilt")?;
            let url = crate::servers_runtime::jar::quilt_server_jar_url(
                &file.mc_version,
                &lv,
                &installer,
            );
            create::create_quilt_server(base, file, &url).await?;
        }
        LoaderKind::Forge | LoaderKind::NeoForge => {
            let lv = create::require_loader_version(file, "forge/neoforge")?;
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
            crate::jre::ensure_jre(&component, app, |_, _, _| {}).await?;
            let java_bin = crate::jre::java_executable_path(&component, app)?;
            create::create_installer_server(base, file, &url, &java_bin, label).await?;
        }
    }
    Ok(())
}

/// Фаза 1 импорта: распаковать/просканировать источник, вернуть превью.
#[tauri::command]
#[specta::specta]
pub fn server_import_inspect(
    app: AppHandle,
    source_path: String,
) -> Result<import::ServerImportPreview> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    import::sweep_stale(&base);
    import::inspect(&base, std::path::Path::new(&source_path))
}

/// Фаза 3: финализировать импорт. Preserve (staged уже запускаем) или
/// reprovision (переустановить загрузчик + скопировать данные).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn server_import_commit(
    app: AppHandle,
    token: String,
    name: String,
    mc_version: String,
    loader: LoaderKind,
    loader_version: Option<String>,
    max_heap_mb: u32,
    eula_accepted: bool,
) -> Result<ServerWithStatus> {
    crate::servers_runtime::eula::require_accepted(eula_accepted)?;
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    // Decide preserve vs reprovision against the staged root.
    let root = import::staged_root(&base, &token)?;
    let preserve = import::detect::can_launch_as_is(&root, loader);

    let id = if preserve {
        import::commit_preserve(
            &base,
            &token,
            &name,
            &mc_version,
            loader,
            loader_version,
            max_heap_mb,
            eula_accepted,
        )?
    } else {
        // Reprovision: build the server.json, provision the loader via create::,
        // then copy the user's data on top, then drop staging.
        let id = format!("srv-{}", crate::instances::ids::new_id());
        let file = import::build_file(
            &id,
            &name,
            &mc_version,
            loader,
            loader_version,
            max_heap_mb,
            eula_accepted,
        );
        provision_loader(&app, &base, &file).await?;
        let p = crate::paths::server_paths(&base, &id);
        import::copy::copy_into_runtime(&root, &p.runtime)?;
        let _ = std::fs::remove_dir_all(import::staging_dir(&base, &token));
        id
    };

    let file = crate::servers_runtime::store::read_server_json(
        &crate::paths::server_paths(&base, &id).json,
    )?;
    Ok(status_of(&base, &file))
}

/// Отменить импорт: удалить staging.
#[tauri::command]
#[specta::specta]
pub fn server_import_cancel(app: AppHandle, token: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    import::cancel(&base, &token)
}

use crate::servers_runtime::serverlog;

/// Список логов сервера (текущий + архивы), отсортированных от новых к старым.
#[tauri::command]
#[specta::specta]
pub fn server_list_logs(app: AppHandle, id: String) -> Result<Vec<serverlog::ServerLogInfo>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    serverlog::list_logs(&crate::paths::server_paths(&base, &id).logs)
}

/// Прочитать файл лога сервера (текущий или архив) с ограничением 1 МиБ.
#[tauri::command]
#[specta::specta]
pub fn server_read_log(app: AppHandle, id: String, file_name: String) -> Result<String> {
    if !serverlog::is_safe_log_name(&file_name) {
        return Err(Error::io("<log>", "invalid filename"));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let path = crate::paths::server_paths(&base, &id).logs.join(&file_name);
    Ok(crate::logs::read::read_with_cap(&path, 1024 * 1024).unwrap_or_default())
}

/// Открыть папку `runtime/logs/` сервера в системном файловом менеджере.
/// Создаёт папку, если она ещё не существует.
#[tauri::command]
#[specta::specta]
pub async fn server_open_logs_folder(app: AppHandle, id: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).logs;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

/// Create a snapshot. If the server is running, flush + pause world saves
/// around the zip so the snapshot isn't torn, then resume. Prunes to keep-N.
#[tauri::command]
#[specta::specta]
pub async fn server_backup_create(app: AppHandle, id: String) -> Result<backup::BackupInfo> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let running = crate::servers_runtime::runtime::is_running(&id);
    if running {
        let _ = crate::servers_runtime::runtime::send_command(&id, "save-all flush").await;
        let _ = crate::servers_runtime::runtime::send_command(&id, "save-off").await;
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let res = backup::create_backup(&base, &id, &stamp);
    if running {
        let _ = crate::servers_runtime::runtime::send_command(&id, "save-on").await;
    }
    res
}

#[tauri::command]
#[specta::specta]
pub fn server_backup_list(app: AppHandle, id: String) -> Result<Vec<backup::BackupInfo>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    backup::list_backups(&base, &id)
}

/// Restore a snapshot. The server MUST be stopped — otherwise the live process
/// holds files open and the restore would corrupt. Auto-backs-up the current
/// state first (safety net), then resets `runtime/` from the snapshot.
#[tauri::command]
#[specta::specta]
pub async fn server_backup_restore(app: AppHandle, id: String, file_name: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    // Safety net: snapshot current state before overwriting it.
    let stamp = format!("prerestore-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    let _ = backup::create_backup(&base, &id, &stamp);
    backup::restore_backup(&base, &id, &file_name)
}

#[tauri::command]
#[specta::specta]
pub fn server_backup_delete(app: AppHandle, id: String, file_name: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    backup::delete_backup(&base, &id, &file_name)
}

// Own server (Plan 11: firewall help):

use crate::servers_runtime::firewall;

/// Windows-Firewall status for a server's port: is there a Lucerna allow-rule?
/// Returns `NotApplicable` immediately on non-Windows hosts.
#[tauri::command]
#[specta::specta]
pub fn server_firewall_status(app: AppHandle, id: String) -> Result<firewall::FirewallState> {
    if !cfg!(target_os = "windows") {
        return Ok(firewall::FirewallState::NotApplicable);
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    let port = crate::servers_runtime::runtime::read_port(&rt);
    let present = match port {
        Some(p) => crate::process::firewall_rule_present(&firewall::rule_name(p)),
        None => false,
    };
    Ok(firewall::status_from(port, present))
}

/// Add an inbound allow rule for the server's port (UAC-elevated). Best-effort:
/// returns Ok once the elevation request is launched; the UAC outcome is not
/// observable from within the launcher process.
#[tauri::command]
#[specta::specta]
pub fn server_firewall_add_rule(app: AppHandle, id: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    let port = crate::servers_runtime::runtime::read_port(&rt).ok_or_else(|| {
        Error::io(
            "<firewall>",
            "server.properties not found — start the server first",
        )
    })?;
    crate::process::firewall_add_rule_elevated(&firewall::rule_name(port), port)
}
