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

/// One entry in `server_list_mods`: the on-disk filename, whether it is set
/// aside (`*.jar.disabled`), and — for disabled jars — why (from the quarantine
/// sidecar), so the UI can label it ("set aside: client-only") instead of
/// inferring everything from the suffix.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerModEntry {
    pub filename: String,
    pub disabled: bool,
    /// Sidecar reason for a disabled jar (e.g. `client_only`); `None` otherwise.
    pub reason: Option<String>,
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
        // (latest log + freshest same-run crash report, pick_diagnosable) so the
        // handled-fix suppression signature matches and the badge clears after a
        // fix. (One bounded per-server read; server_list is infrequent.)
        let (content, crash) = read_diagnosable(&rp);
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
    // Capture the busy port before the finding is moved, so a PortInUse can carry
    // a genuinely-free suggested port for the "Use port N" one-click fix.
    let busy_port = match finding {
        preflight::PreflightFinding::PortInUse(p) => Some(p),
        _ => None,
    };
    let mut diag = crate::logs::diagnose::server::diagnosis_from_preflight(finding);
    if let Some(busy) = busy_port {
        diag.suggested_port = preflight::next_free_port(busy.saturating_add(1), busy);
    }
    Some(diag)
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
    // Suppress the now-stale log diagnosis so re-diagnose doesn't re-fire the
    // banner from the same log after the fix (parity with the class-B fixes).
    mark_current_log_handled(&p);
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
    let p = crate::paths::server_paths(&base, &id);
    crate::servers_runtime::pid::clear_pid(&p.pid);
    // Suppress the now-stale session-lock log so re-diagnose doesn't re-fire it.
    mark_current_log_handled(&p);
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
    // The port we're leaving — its firewall allow-rule (if any) is now stale.
    let old_port = crate::servers_runtime::runtime::read_port(&p.runtime);
    let mut props = crate::servers_runtime::properties::ServerProperties::parse(&raw);
    props.set_validated("server-port", &port.to_string())?;
    std::fs::create_dir_all(&p.runtime)
        .map_err(|e| Error::io(p.runtime.display().to_string(), e))?;
    std::fs::write(&props_path, props.serialize())
        .map_err(|e| Error::io("<server.properties>", e))?;
    // Migrate the firewall rule: remove the old port's allow-rule (if present) so
    // changing the port doesn't leave a stale open-port rule behind. The new
    // port's rule is added on demand via `server_firewall_add_rule`.
    if let Some(old) = old_port {
        if old != port {
            remove_firewall_rule_for_port(&p.root, old);
        }
    }
    // Suppress the now-stale port-conflict log so re-diagnose doesn't re-fire the
    // banner from the same FAILED-TO-BIND log after the port has been changed.
    mark_current_log_handled(&p);
    Ok(())
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
    if crate::servers_runtime::upload_control::upload_is_active(&id) {
        return Err(crate::error::Error::ServerUploadInProgress { id });
    }
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
    if crate::servers_runtime::upload_control::upload_is_active(&id) {
        return Err(crate::error::Error::ServerUploadInProgress { id });
    }
    crate::servers_runtime::runtime::restart(&app, &id).await
}

/// Отправить консольную команду на stdin работающего сервера.
#[tauri::command]
#[specta::specta]
pub async fn server_send_command(id: String, line: String) -> Result<()> {
    crate::servers_runtime::runtime::send_command(&id, &line).await
}

/// Remove the firewall allow-rule for a single `port` when it is present, then
/// forget it from the tracking sidecar. Best-effort; gating on presence means UAC
/// is only prompted when there is actually a rule to remove.
fn remove_firewall_rule_for_port(root: &std::path::Path, port: u16) {
    let name = crate::servers_runtime::firewall::rule_name(port);
    if crate::process::firewall_rule_present(&name) {
        let _ = crate::process::firewall_remove_rule_elevated(&name);
    }
    crate::servers_runtime::firewall::forget_port(root, port);
}

/// Best-effort removal of EVERY Windows firewall allow-rule this server left
/// behind, so deleting it never leaves a stale open-port rule. Removes each rule
/// recorded in the tracking sidecar PLUS the current `server.properties` port (in
/// case the sidecar predates tracking / was never written). No-op on non-Windows
/// or when no rule is present.
fn remove_firewall_rules_on_delete(root: &std::path::Path, runtime: &std::path::Path) {
    let mut ports = crate::servers_runtime::firewall::recorded_ports(root);
    if let Some(cur) = crate::servers_runtime::runtime::read_port(runtime) {
        if !ports.contains(&cur) {
            ports.push(cur);
        }
    }
    for port in ports {
        remove_firewall_rule_for_port(root, port);
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
    // Remove every firewall allow-rule we may have added for this server (all
    // tracked ports + the current one) so none linger after the server is gone.
    remove_firewall_rules_on_delete(&p.root, &p.runtime);
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

/// Перечислить `.jar` и `.jar.disabled` файлы в папке `mods/` сервера как
/// [`ServerModEntry`] (имя + флаг `disabled` + причина из sidecar карантина).
/// Отсортировано по имени. Если папка отсутствует — пустой список.
#[tauri::command]
#[specta::specta]
pub fn server_list_mods(app: AppHandle, id: String) -> Result<Vec<ServerModEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let reasons = crate::servers_runtime::quarantine::read_reasons(&mods);
    let mut names = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&mods) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let low = n.to_ascii_lowercase();
            if low.ends_with(".jar") || low.ends_with(".jar.disabled") {
                names.push(n);
            }
        }
    }
    names.sort();
    Ok(names
        .into_iter()
        .map(|filename| {
            let disabled = filename.to_ascii_lowercase().ends_with(".jar.disabled");
            let reason = if disabled {
                reasons.get(&filename).cloned()
            } else {
                None
            };
            ServerModEntry {
                filename,
                disabled,
                reason,
            }
        })
        .collect())
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
    // Latest run's diagnosable input: live log + freshest SAME-RUN crash report
    // (a stale report from a prior run is gated out so it can't mis-diagnose the
    // current crash). Phase 2: the richer of the two is diagnosed.
    let (content, crash_text) = read_diagnosable(&p);
    if content.is_empty() && crash_text.is_none() {
        if let Some(d) = preflight_diagnosis(&p, &id) {
            return Ok(d);
        }
        // No log, no fresh crash report, no pre-spawn blocker — but the last run
        // may still have crashed (e.g. a Windows process-init failure that wrote
        // nothing). Surface the exit code instead of a silent "all clear".
        if let Some(code) = crate::servers_runtime::exit_state::read(&p.runtime) {
            if let Some(d) = crate::logs::diagnose::server::diagnosis_from_exit_code(code) {
                return Ok(d);
            }
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
            suggested_port: None,
            exit_code: None,
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
        // The log/crash text didn't match any pattern and there's no pre-spawn
        // blocker — but if the run still exited with a crash code, explain that
        // rather than returning an unhelpful "no diagnosis".
        if let Some(code) = crate::servers_runtime::exit_state::read(&p.runtime) {
            if let Some(d) = crate::logs::diagnose::server::diagnosis_from_exit_code(code) {
                return Ok(d);
            }
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
    let mut port_in_use = None;
    let mut suggested_port = None;
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
            ServerRepairTag::ChangePort => {
                // Log-detected port conflict (FAILED TO BIND). The log doesn't name
                // a free port, so report the configured port AND probe for the next
                // genuinely-free one so "Use port N" can't suggest the current (a
                // no-op) or another busy port.
                let cur = crate::servers_runtime::runtime::read_port(&p.runtime).unwrap_or(25565);
                port_in_use = Some(cur);
                suggested_port =
                    crate::servers_runtime::preflight::next_free_port(cur.saturating_add(1), cur);
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
        port_in_use,
        orphan_pid,
        corrupt_jar,
        suggested_heap_mb,
        conflict_mods,
        suggested_port,
        exit_code: None,
    })
}

/// Diagnosable cap for a server's log / crash report (1 MB each).
const DIAG_READ_CAP: u64 = 1024 * 1024;

/// Creation time of the current `server-latest.log` — the moment the latest run
/// started. `start()` rotates the prior log to an archive and creates a fresh
/// `server-latest.log`, so its creation time anchors "this run". For a crash
/// that produced no output the file is created and never written again, so the
/// anchor is exact. `None` when the log is absent or the OS reports no creation
/// time (callers then fall back to considering any crash report).
///
/// Rotation uses `rename` (not delete + recreate), so the fresh log is always
/// created after the old name is freed — no NTFS creation-time "tunneling" to
/// stale the anchor. If the rotation strategy ever changes to delete+recreate,
/// revisit this (a 15s tunnel window could reuse the prior creation time).
fn latest_run_anchor(logs_dir: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(logs_dir.join(crate::servers_runtime::serverlog::LATEST))
        .ok()
        .and_then(|m| m.created().ok())
}

/// Read the newest `crash-*.txt` (by mtime) under `dir`, capped at `cap` bytes,
/// **but only if it belongs to the current run** — i.e. its mtime is at/after
/// `anchor` (the latest run's start). This stops a days-old crash report from a
/// previous run being diagnosed as the latest crash (which would surface a
/// misleading banner for an unrelated failure). When `anchor` is `None` the
/// freshness gate is skipped (back-compat for servers without a readable log).
fn newest_crash_text(
    dir: &std::path::Path,
    cap: u64,
    anchor: Option<std::time::SystemTime>,
) -> Option<String> {
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("crash-") || !name.ends_with(".txt") {
            continue;
        }
        let Ok(m) = e.metadata().and_then(|md| md.modified()) else {
            continue;
        };
        // Skip crash reports older than this run's start (stale from a prior run).
        if let Some(a) = anchor {
            if m < a {
                continue;
            }
        }
        if newest.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
            newest = Some((m, e.path()));
        }
    }
    let (_, path) = newest?;
    crate::logs::read::read_with_cap(&path, cap).ok()
}

/// The diagnosable inputs for a server's latest run: the live `server-latest.log`
/// contents and the freshest *same-run* crash report. Single source so the
/// diagnosis command, the handled-signature marker, and the list badge all see
/// the identical input (and therefore agree on the log signature).
fn read_diagnosable(p: &crate::paths::ServerPaths) -> (String, Option<String>) {
    let content =
        crate::logs::read::read_with_cap(&p.logs.join("server-latest.log"), DIAG_READ_CAP)
            .unwrap_or_default();
    let crash = newest_crash_text(
        &p.runtime.join("crash-reports"),
        DIAG_READ_CAP,
        latest_run_anchor(&p.logs),
    );
    (content, crash)
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
    let (content, crash) = read_diagnosable(p);
    let input = crate::logs::diagnose::server::pick_diagnosable(&content, crash.as_deref());
    if input.is_empty() {
        return;
    }
    // Only record a handled signature when the log/crash text actually diagnoses
    // a server problem. A class-A fix (change-port / stop-orphan / accept-EULA)
    // reached from a PREFLIGHT finding can coexist with an unrelated,
    // undiagnosable log; marking that log handled would wrongly suppress a later
    // real diagnosis. A log-derived fix always has a diagnosis here, so it is
    // still marked (parity with the class-B heap/jar/dep fixes).
    if crate::logs::diagnose::server::diagnose_server_log(input).is_none() {
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
    let prev_target = file
        .upload
        .as_ref()
        .map(crate::servers_runtime::upload_manifest::target_of);
    file.upload = Some(config.clone());
    crate::servers_runtime::store::write_server_json(&p.json, &file)?;
    // A changed target invalidates any in-progress resume manifest (the planned
    // remote no longer matches what was partially uploaded).
    let new_target = crate::servers_runtime::upload_manifest::target_of(&config);
    if prev_target.as_ref() != Some(&new_target) {
        crate::servers_runtime::upload_manifest::delete_manifest(
            &crate::servers_runtime::upload_manifest::manifest_path(&base, &id),
        );
    }
    if let Some(pw) = password {
        crate::accounts::keychain::store(&crate::accounts::keychain::sftp_password_key(&id), &pw)?;
    }
    Ok(())
}

/// Resolve the secret an upload should authenticate with (#C, transient-secret
/// path). A `transient` password (typed this session with "Save password" off)
/// takes precedence over the keyring and is never persisted. With no transient
/// secret we fall back to the keyring. Password auth with neither is an error;
/// key auth with neither is fine (an unencrypted key → empty passphrase).
fn resolve_upload_secret(
    method: crate::servers_runtime::transfer::UploadAuthMethod,
    transient: Option<String>,
    stored: Option<String>,
) -> Result<String> {
    if let Some(pw) = transient {
        return Ok(pw); // explicit Some("") is honoured (blank key passphrase)
    }
    match (method, stored) {
        (crate::servers_runtime::transfer::UploadAuthMethod::Password, None) => {
            Err(crate::error::Error::UploadNotConfigured)
        }
        (_, s) => Ok(s.unwrap_or_default()),
    }
}

/// Загрузить серверный `runtime/` на SFTP-хост. Сервер должен быть остановлен.
///
/// `password` — транзитный секрет, введённый в этой сессии при выключенной
/// опции «Сохранить пароль»; если передан, используется вместо связки ключей
/// и **никогда не сохраняется**. При `None` секрет берётся из keyring как
/// обычно.
///
/// При первом подключении или изменении ключа хоста возвращает ошибку
/// `SftpHostKeyMismatch`, если `accept_new_host_key == false`. При `true`
/// доверяет новому ключу и сохраняет его отпечаток в `server.json`.
#[tauri::command]
#[specta::specta]
pub async fn server_upload(
    app: AppHandle,
    id: String,
    accept_new_host_key: bool,
    skip_worlds: bool,
    password: Option<String>,
    resume: bool,
) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(crate::error::Error::ServerAlreadyRunning { id });
    }
    if crate::servers_runtime::upload_control::upload_is_active(&id) {
        return Err(crate::error::Error::ServerUploadInProgress { id });
    }
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let cfg = file
        .upload
        .ok_or(crate::error::Error::UploadNotConfigured)?;
    let auth = crate::servers_runtime::transfer::read_upload_auth(&base, &id);
    let stored =
        crate::accounts::keychain::retrieve(&crate::accounts::keychain::sftp_password_key(&id))?;
    let secret = resolve_upload_secret(auth.method, password, stored)?;
    let cancel = crate::servers_runtime::upload_control::upload_begin(&id);
    let result = crate::servers_runtime::transfer::upload_server(
        &app,
        &id,
        &cfg,
        &auth,
        &secret,
        accept_new_host_key,
        skip_worlds,
        &cancel,
        resume,
    )
    .await;
    crate::servers_runtime::upload_control::upload_end(&id);
    result
}

/// Resumable-upload snapshot for the Hosting tab. `resumable` is true iff an
/// unfinished manifest exists for the CURRENT configured target.
///
/// `bytes_total` is `f64` on the wire — specta-typescript has no `u64`, so
/// byte counts cross the boundary as `f64` (same convention as `created_unix_ms`
/// in `schema.rs`). Values stay within 2^53 for any realistic server.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct UploadResumeState {
    pub resumable: bool,
    pub files_total: u32,
    pub files_done: u32,
    pub bytes_total: f64,
}

/// Report whether a resumable upload exists for the server's current target.
/// Pure read — no connection is made, no secret is touched. Used to show the
/// "Продолжить заливку" affordance on the Hosting tab.
#[tauri::command]
#[specta::specta]
pub fn server_upload_resume_state(app: AppHandle, id: String) -> Result<UploadResumeState> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let none = UploadResumeState {
        resumable: false,
        files_total: 0,
        files_done: 0,
        bytes_total: 0.0,
    };
    let p = crate::paths::server_paths(&base, &id);
    let Ok(file) = crate::servers_runtime::store::read_server_json(&p.json) else {
        return Ok(none);
    };
    let Some(cfg) = file.upload else {
        return Ok(none);
    };
    let manifest_path = crate::servers_runtime::upload_manifest::manifest_path(&base, &id);
    let Some(m) = crate::servers_runtime::upload_manifest::read_manifest(&manifest_path) else {
        return Ok(none);
    };
    // Same target, and not already fully done (a complete manifest is deleted on
    // success, but guard anyway).
    if m.target != crate::servers_runtime::upload_manifest::target_of(&cfg) {
        return Ok(none);
    }
    let files_total = m.files.len() as u32;
    let files_done = m.files.iter().filter(|f| f.done).count() as u32;
    if files_done >= files_total && files_total > 0 {
        return Ok(none);
    }
    Ok(UploadResumeState {
        resumable: true,
        files_total,
        files_done,
        bytes_total: m.files.iter().map(|f| f.size).sum::<u64>() as f64,
    })
}

/// Size/free-space preflight for an upload (#K): total bytes of the selected set
/// (honouring `skip_worlds`) plus remote free space when the server advertises
/// the `statvfs` SFTP extension. Transfers nothing. A host-key mismatch is
/// surfaced; other connection blips degrade to "free space unknown".
///
/// Secret resolution mirrors `server_upload` (no transient password: a preflight
/// reads the keyring; password auth with no stored secret is `UploadNotConfigured`).
#[tauri::command]
#[specta::specta]
pub async fn server_upload_preflight(
    app: AppHandle,
    id: String,
    accept_new_host_key: bool,
    skip_worlds: bool,
) -> Result<crate::servers_runtime::transfer::UploadPreflight> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let cfg = file
        .upload
        .ok_or(crate::error::Error::UploadNotConfigured)?;
    let auth = crate::servers_runtime::transfer::read_upload_auth(&base, &id);
    let stored =
        crate::accounts::keychain::retrieve(&crate::accounts::keychain::sftp_password_key(&id))?;
    let secret = resolve_upload_secret(auth.method, None, stored)?;
    crate::servers_runtime::transfer::upload_preflight(
        &app,
        &id,
        &cfg,
        &auth,
        &secret,
        accept_new_host_key,
        skip_worlds,
    )
    .await
}

/// Запросить отмену активной заливки на хостинг (no-op, если её нет).
/// Частично залитые файлы остаются на хосте (докачка — отдельная фича).
#[tauri::command]
#[specta::specta]
pub fn server_cancel_upload(id: String) -> Result<()> {
    crate::servers_runtime::upload_control::upload_cancel(&id);
    Ok(())
}

/// Экспортировать серверный `runtime/` в ZIP-архив по пути `dest_path`.
/// Исключает `logs/` и `installer.jar` (те же правила, что у SFTP-загрузки).
#[tauri::command]
#[specta::specta]
pub fn server_export_zip(app: AppHandle, id: String, dest_path: String) -> Result<()> {
    // A live server holds world region files open and mutates them mid-write, so
    // zipping runtime/ while it runs can produce a torn archive. Refuse until the
    // server is stopped (parity with restore/upload, which also require stopped).
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(crate::error::Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    crate::servers_runtime::transfer::export_zip(&p.runtime, std::path::Path::new(&dest_path))
}

/// Read the server's SFTP host-key fingerprint for first-connect verification
/// (#24). Connects and captures the key at key-exchange — NO password or key is
/// sent and nothing is uploaded; the session is dropped immediately. The user
/// verifies the returned fingerprint against their provider before trusting it.
#[tauri::command]
#[specta::specta]
pub async fn server_host_key_preview(
    app: AppHandle,
    id: String,
) -> Result<crate::servers_runtime::transfer::HostKeyPreview> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let cfg = file
        .upload
        .ok_or(crate::error::Error::UploadNotConfigured)?;
    crate::servers_runtime::transfer::preview_host_key(&cfg).await
}

/// Read the server's SFTP auth method (#28). Absent → password (back-compat).
#[tauri::command]
#[specta::specta]
pub fn server_get_upload_auth(
    app: AppHandle,
    id: String,
) -> Result<crate::servers_runtime::transfer::UploadAuth> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    Ok(crate::servers_runtime::transfer::read_upload_auth(
        &base, &id,
    ))
}

/// Set the server's SFTP auth method (#28). When `auth.method == Key`, the
/// `password` field of the upload form is treated as the key passphrase and
/// stored in the keyring exactly like a password (never in `auth.json`).
#[tauri::command]
#[specta::specta]
pub fn server_set_upload_auth(
    app: AppHandle,
    id: String,
    auth: crate::servers_runtime::transfer::UploadAuth,
) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    crate::servers_runtime::transfer::write_upload_auth(&base, &id, &auth)
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

/// Public-address snapshot for the hosting view (#6, contract C3): a primary LAN
/// address, the detected public IP (for manual port-forward guidance), the
/// server port, and online-mode. `public_ip` is `None` when the on-demand echo
/// can't be reached (offline / host down) — LAN + port stay useful regardless.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerPublicAddress {
    /// Primary LAN IPv4 (first detected), or empty when none is available.
    pub lan: String,
    /// Detected public IP, or `None` when the echo lookup failed.
    pub public_ip: Option<String>,
    /// Configured server port (Mojang's default 25565 when unset).
    pub port: u16,
    /// `online-mode` from `server.properties` (defaults true when unset).
    pub online_mode: bool,
}

/// ipify returns the caller's public IP as plain text. Validate it actually
/// parses as an IP address before trusting the body (defends against an error
/// page / unexpected payload leaking into the UI). Returns the canonical form.
fn valid_public_ip(body: &str) -> Option<String> {
    body.trim()
        .parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| ip.to_string())
}

/// Detect the server's public address for port-forward guidance (#6). The
/// public-IP lookup is **user-initiated and on-demand** (the user opens the
/// hosting view and asks) — never automatic — and goes through the `network::`
/// chokepoint to the allowlisted `api.ipify.org`. Per maintainer default #6 this
/// is detection + manual guidance only: NO UPnP / automatic port mapping.
#[tauri::command]
#[specta::specta]
pub async fn server_public_address(app: AppHandle, id: String) -> Result<ServerPublicAddress> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    let (port, online_mode) = match std::fs::read_to_string(rt.join("server.properties")) {
        Ok(raw) => {
            let props = crate::servers_runtime::properties::ServerProperties::parse(&raw);
            let port = props
                .get("server-port")
                .and_then(|v| v.parse().ok())
                .unwrap_or(25565);
            let online_mode = props
                .get("online-mode")
                .map(|v| v != "false")
                .unwrap_or(true);
            (port, online_mode)
        }
        Err(_) => (25565, true),
    };
    let lan = crate::process::local_ipv4_addresses()
        .into_iter()
        .next()
        .unwrap_or_default();
    // Best-effort: a failed lookup (offline, blocked) yields None, not an error.
    let public_ip =
        match crate::network::get_text("https://api.ipify.org", "server_public_address").await {
            Ok(body) => valid_public_ip(&body),
            Err(e) => {
                crate::diag!("server_public_address: public-IP echo failed: {e}");
                None
            }
        };
    Ok(ServerPublicAddress {
        lan,
        public_ip,
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
    // Enforce name validation at the IPC boundary (parity with server_create):
    // reject empty / control-char / duplicate names before committing the import.
    let name = store::validate_name(&name, &store::list_all(&base)?, None)?;
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
        // then lay down the user's data, then drop staging. A server PACK (#10)
        // materializes its data differently — Modrinth downloads its server
        // files; a bundled CurseForge pack copies + applies overrides; a CF
        // client manifest (mods are download refs) is out of scope here.
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
        match import::pack::detect_pack(&root) {
            Some(import::pack::PackKind::Modrinth) => {
                let pack = import::pack::parse_modrinth(&root)?;
                import::pack::materialize_modrinth(&pack, &root, &p.runtime).await?;
            }
            Some(import::pack::PackKind::Curseforge) => {
                let cf = import::pack::parse_cf(&root)?;
                if !cf.bundled_mods {
                    return Err(Error::ServerImportInvalidArchive {
                        details: "This CurseForge pack references its mods as downloads rather \
                                  than bundling them. Import it as a client modpack first, then \
                                  use \"Create server from instance\"."
                            .to_string(),
                    });
                }
                import::copy::copy_into_runtime(&root, &p.runtime)?;
                import::pack::apply_overrides(&root, &p.runtime)?;
            }
            None => {
                import::copy::copy_into_runtime(&root, &p.runtime)?;
            }
        }
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
    // Safety net: snapshot current state before overwriting it. If the snapshot
    // FAILS (e.g. disk full), ABORT — restoring would `remove_dir_all` the live
    // runtime with no recoverable copy of the state we're about to destroy (#26).
    // Protect the restore target from the pre-restore snapshot's keep-N prune: if
    // the backup set is already at the cap and `file_name` is the oldest, an
    // unprotected prune would delete the very zip we're about to restore from.
    let stamp = format!("prerestore-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    backup::create_backup_protecting(&base, &id, &stamp, Some(&file_name))?;
    backup::restore_backup(&base, &id, &file_name)
}

/// Read the server's automatic-backup policy (#29). Absent → disabled default.
#[tauri::command]
#[specta::specta]
pub fn server_backup_policy_get(app: AppHandle, id: String) -> Result<backup::BackupPolicy> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    Ok(backup::read_policy(&base, &id))
}

/// Set the server's automatic-backup policy (#29) and (re)arm the session
/// interval scheduler. Setting `enabled=false` or a zero interval cancels any
/// running scheduler for this server (via the generation bump below).
///
/// Scope note: the scheduler is **session-scoped** — it runs while the launcher
/// is open and the server is running. Cross-restart durability and an on-exit
/// snapshot need the server-lifecycle exit hook (S1-owned) and are a documented
/// follow-up; the spec's "and/or interval" sanctions the interval-only delivery.
#[tauri::command]
#[specta::specta]
pub fn server_backup_policy_set(
    app: AppHandle,
    id: String,
    policy: backup::BackupPolicy,
) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    // The UI owns `enabled` + `interval_minutes` only; `last_run_unix_ms` is
    // bookkeeping the scheduler stamps. Preserve the stored value so saving the
    // policy doesn't reset the schedule to "never run" (immediately due).
    let mut policy = policy;
    policy.last_run_unix_ms = backup::read_policy(&base, &id).last_run_unix_ms;
    backup::write_policy(&base, &id, &policy)?;
    // Bump this server's scheduler generation. Any task spawned by a prior set()
    // sees the mismatch on its next tick and exits, so we never leak overlapping
    // schedulers or honour a stale interval.
    let generation = {
        let mut gens = backup_scheduler_generations()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let g = gens.entry(id.clone()).or_insert(0);
        *g += 1;
        *g
    };
    if policy.enabled && policy.interval_minutes > 0 {
        spawn_backup_scheduler(app, id, generation, policy.interval_minutes);
    }
    Ok(())
}

/// Per-server scheduler generation counter. A `server_backup_policy_set` call
/// bumps the entry; the matching background task exits once its captured
/// generation no longer matches (cancel / supersede). Poison-tolerant.
fn backup_scheduler_generations() -> &'static std::sync::Mutex<HashMap<String, u64>> {
    static G: std::sync::OnceLock<std::sync::Mutex<HashMap<String, u64>>> =
        std::sync::OnceLock::new();
    G.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Spawn the session interval scheduler for one server. Sleeps the interval,
/// then (while it is still the current generation) snapshots a *running* server,
/// flushing + pausing world saves around the zip so the snapshot isn't torn —
/// the same dance as `server_backup_create`. Idle servers are skipped (their
/// world isn't changing).
fn spawn_backup_scheduler(app: AppHandle, id: String, generation: u64, interval_minutes: u32) {
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_secs(interval_minutes.max(1) as u64 * 60);
        loop {
            tokio::time::sleep(interval).await;
            let current = backup_scheduler_generations()
                .lock()
                .ok()
                .and_then(|gens| gens.get(&id).copied())
                .unwrap_or(0);
            if current != generation {
                return; // superseded or cancelled by a later policy-set
            }
            if !crate::servers_runtime::runtime::is_running(&id) {
                continue; // idle server: nothing new to snapshot
            }
            let Ok(base) = crate::paths::app_dir(&app) else {
                continue;
            };
            let t = chrono::Utc::now();
            let now = t.timestamp_millis() as f64;
            let stamp = format!("auto-{}", t.format("%Y%m%d-%H%M%S"));
            let _ = crate::servers_runtime::runtime::send_command(&id, "save-all flush").await;
            let _ = crate::servers_runtime::runtime::send_command(&id, "save-off").await;
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            let res = crate::servers_runtime::backup::maybe_auto_backup(&base, &id, now, &stamp);
            let _ = crate::servers_runtime::runtime::send_command(&id, "save-on").await;
            if let Err(e) = res {
                crate::diag!("auto-backup: {id}: {e}");
            }
        }
    });
}

/// Re-arm the session interval backup scheduler for every server whose persisted
/// `backup-policy.json` is enabled with a positive interval. The scheduler is
/// session-scoped: it is only spawned by `server_backup_policy_set`, so after a
/// launcher restart an enabled policy stops producing snapshots until the user
/// re-saves it. Called once from `lib.rs` setup so enabled policies survive a
/// restart.
///
/// Reuses the same per-server generation map a later `server_backup_policy_set`
/// bumps, so a policy edit made after this rearm still supersedes the task
/// spawned here (the older generation exits on its next tick). Best-effort: a
/// missing servers root or an unreadable policy is skipped.
pub fn rearm_backup_schedulers(app: &AppHandle) {
    let Ok(base) = crate::paths::app_dir(app) else {
        return;
    };
    let servers = match store::list_all(&base) {
        Ok(s) => s,
        Err(e) => {
            crate::diag!("rearm_backup_schedulers: list servers failed: {e}");
            return;
        }
    };
    for file in servers {
        let policy = backup::read_policy(&base, &file.id);
        if !policy.enabled || policy.interval_minutes == 0 {
            continue;
        }
        // Bump this server's generation (same mechanism as policy_set) so a later
        // policy_set supersedes the task we spawn here.
        let generation = {
            let mut gens = backup_scheduler_generations()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let g = gens.entry(file.id.clone()).or_insert(0);
            *g += 1;
            *g
        };
        spawn_backup_scheduler(app.clone(), file.id, generation, policy.interval_minutes);
    }
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
    crate::process::firewall_add_rule_elevated(&firewall::rule_name(port), port)?;
    // Record the port so `server_delete` (and a later `server_change_port`) can
    // remove every rule we created, not just the current-port one.
    let root = crate::paths::server_paths(&base, &id).root;
    firewall::record_added_port(&root, port);
    Ok(())
}

// Own server (#9, C4: whitelist / ops editor):

use crate::servers_runtime::whitelist;

/// Mojang's public username→profile lookup payload.
#[derive(serde::Deserialize)]
struct MojangProfile {
    id: String,
    name: String,
}

/// Read a server's `online-mode` (defaults true when unset/missing).
fn server_online_mode(runtime: &std::path::Path) -> bool {
    match std::fs::read_to_string(runtime.join("server.properties")) {
        Ok(raw) => crate::servers_runtime::properties::ServerProperties::parse(&raw)
            .get("online-mode")
            .map(|v| v != "false")
            .unwrap_or(true),
        Err(_) => true,
    }
}

/// Resolve a player `name` to the (uuid, canonical-name) the server will match
/// against. On an **online-mode** server we must use the real Mojang UUID, so we
/// look it up via the allowlisted Mojang profile API (a 404 / unreachable host
/// surfaces as an error — adding a wrong UUID would silently fail to whitelist).
/// On an **offline-mode** server we derive the deterministic offline UUID (no
/// network), matching how the server identifies offline players.
async fn resolve_player_identity(
    runtime: &std::path::Path,
    name: &str,
) -> Result<(String, String)> {
    let name = name.trim();
    // Minecraft usernames are 1–16 chars of `[A-Za-z0-9_]`. Validate before
    // interpolating into the Mojang lookup URL — a `/`, `?`, or `#` would
    // otherwise alter the request path/query (defense in depth; the allowlist
    // guards only the host, not the path).
    if name.is_empty()
        || name.len() > 16
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(Error::io("<whitelist>", "invalid Minecraft username"));
    }
    if server_online_mode(runtime) {
        let url = format!("https://api.mojang.com/users/profiles/minecraft/{name}");
        let profile: MojangProfile = crate::network::get_json(&url, "server_whitelist_resolve")
            .await
            .map_err(|e| Error::io("<whitelist>", format!("could not look up '{name}': {e}")))?;
        let uuid = crate::accounts::microsoft::mc_services::hyphenate_uuid(&profile.id)?;
        Ok((uuid, profile.name))
    } else {
        let uuid = crate::accounts::offline::derive_offline_uuid(name).to_string();
        Ok((uuid, name.to_string()))
    }
}

#[tauri::command]
#[specta::specta]
pub fn server_whitelist_list(app: AppHandle, id: String) -> Result<Vec<whitelist::WhitelistEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    whitelist::list_whitelist(&rt)
}

/// Whitelist a player by name (#9). Resolves the correct UUID for the server's
/// online-mode, then writes the entry. Pair with the `white-list=true` toggle in
/// the UI so enabling the whitelist never locks the owner out (the lockout fix).
#[tauri::command]
#[specta::specta]
pub async fn server_whitelist_add(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Vec<whitelist::WhitelistEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    let (uuid, canonical) = resolve_player_identity(&rt, &name).await?;
    whitelist::add_whitelist(
        &rt,
        whitelist::WhitelistEntry {
            uuid,
            name: canonical,
        },
    )
}

#[tauri::command]
#[specta::specta]
pub fn server_whitelist_remove(
    app: AppHandle,
    id: String,
    key: String,
) -> Result<Vec<whitelist::WhitelistEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    whitelist::remove_whitelist(&rt, &key)
}

#[tauri::command]
#[specta::specta]
pub fn server_ops_list(app: AppHandle, id: String) -> Result<Vec<whitelist::OpEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    whitelist::list_ops(&rt)
}

/// Grant a player operator status by name (#9). Resolves the UUID for the
/// server's online-mode and writes a level-4 op entry.
#[tauri::command]
#[specta::specta]
pub async fn server_ops_add(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Vec<whitelist::OpEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    let (uuid, canonical) = resolve_player_identity(&rt, &name).await?;
    whitelist::add_op(
        &rt,
        whitelist::OpEntry {
            uuid,
            name: canonical,
            level: 4,
            bypasses_player_limit: false,
        },
    )
}

#[tauri::command]
#[specta::specta]
pub fn server_ops_remove(
    app: AppHandle,
    id: String,
    key: String,
) -> Result<Vec<whitelist::OpEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let rt = crate::paths::server_paths(&base, &id).runtime;
    whitelist::remove_op(&rt, &key)
}

// Own server (Plan 2 / S2: mod content management — browse-install, enable,
// local install, datapacks):

/// Install a chosen mod version + its required dependency closure into the
/// server's `mods/`. Resolves the server's mc_version + loader from
/// `server.json`, then reuses the shared install kernel
/// ([`crate::commands::install_version_into_dir`]). Server must be stopped.
/// Returns the jars written + any dependency that could not be resolved.
#[tauri::command]
#[specta::specta]
pub async fn server_install_mod(
    app: AppHandle,
    id: String,
    source: crate::mods::platform::ModSource,
    project_id: String,
    version_id: String,
) -> Result<crate::mods::dep_resolve::InstallMissingReport> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    crate::commands::install_version_into_dir(
        &base,
        &p.mods,
        source,
        &project_id,
        &version_id,
        &file.mc_version,
        file.loader,
    )
    .await
}

/// Re-enable a set-aside mod: rename `<name>.jar.disabled` → `<name>.jar`.
/// Inverse of `server_disable_mods`. Idempotent (absent → `Ok`). Rejects unsafe
/// filenames / path escapes. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub fn server_enable_mod(app: AppHandle, id: String, filename: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::io("<mod>", "invalid filename"));
    }
    let stripped = match filename.strip_suffix(".disabled") {
        Some(s) => s.to_string(),
        None => return Err(Error::io("<mod>", "not a disabled mod")),
    };
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let src = mods.join(&filename);
    let dst = mods.join(&stripped);
    if !src.starts_with(&mods) || !dst.starts_with(&mods) {
        return Err(Error::io("<mod>", "path escapes mods dir"));
    }
    match std::fs::rename(&src, &dst) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(src.display().to_string(), e)),
    }
    // Drop the now-stale quarantine sidecar entry so the row stops showing a
    // "set aside" reason. Best-effort: a missing/locked sidecar is non-fatal.
    crate::servers_runtime::quarantine::forget_reason(&mods, &filename);
    Ok(())
}

/// Install a local mod `.jar` (chosen via the file picker) into the server's
/// `mods/`. Mirrors the client `mods_install_local` (path-based — no heavy bytes
/// over IPC). Validates the jar is readable and the destination name is safe.
/// Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_install_local(app: AppHandle, id: String, jar_path: String) -> Result<String> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let filename = std::path::Path::new(&jar_path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::io("<mod>", "dropped path has no filename"))?;
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::io("<mod>", "invalid filename"));
    }
    if !filename.to_ascii_lowercase().ends_with(".jar") {
        return Err(Error::io("<mod>", "mod must be a .jar"));
    }
    let bytes = tokio::fs::read(&jar_path)
        .await
        .map_err(|e| Error::io(jar_path.clone(), e))?;
    // Validate it parses as a jar (a zip) before committing — reject junk.
    if crate::mods::local::read_jar_meta(&bytes).is_err() {
        return Err(Error::io("<mod>", "not a valid mod jar"));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    tokio::fs::create_dir_all(&mods)
        .await
        .map_err(|e| Error::io(mods.display().to_string(), e))?;
    let dest = mods.join(&filename);
    if !dest.starts_with(&mods) {
        return Err(Error::io("<mod>", "path escapes mods dir"));
    }
    tokio::fs::write(&dest, &bytes)
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;
    Ok(filename)
}

/// Raw `server.properties` text for a server (empty when absent) — used to
/// resolve the `level-name` (datapacks live under `runtime/<level>/datapacks/`).
fn server_props_raw(p: &crate::paths::ServerPaths) -> String {
    std::fs::read_to_string(p.runtime.join("server.properties")).unwrap_or_default()
}

/// List the datapack archives installed for a server's world.
#[tauri::command]
#[specta::specta]
pub fn server_list_datapacks(app: AppHandle, id: String) -> Result<Vec<String>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let dir = crate::servers_runtime::datapacks::datapacks_dir(&p.runtime, &server_props_raw(&p));
    Ok(crate::servers_runtime::datapacks::list_datapacks(&dir))
}

/// Install a datapack `.zip` (chosen via the file picker) into the server's
/// world `datapacks/`. Validates the zip carries a root `pack.mcmeta`. Returns
/// the installed filename. Server must be stopped (live worlds hold files open).
#[tauri::command]
#[specta::specta]
pub fn server_install_datapack(app: AppHandle, id: String, zip_path: String) -> Result<String> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let dir = crate::servers_runtime::datapacks::datapacks_dir(&p.runtime, &server_props_raw(&p));
    crate::servers_runtime::datapacks::install_datapack(&dir, std::path::Path::new(&zip_path))
}

/// Remove a datapack archive from a server's world `datapacks/`. Idempotent.
/// Server must be stopped.
#[tauri::command]
#[specta::specta]
pub fn server_remove_datapack(app: AppHandle, id: String, filename: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let dir = crate::servers_runtime::datapacks::datapacks_dir(&p.runtime, &server_props_raw(&p));
    crate::servers_runtime::datapacks::remove_datapack(&dir, &filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_public_ip_accepts_ipv4_and_trims() {
        assert_eq!(
            valid_public_ip("203.0.113.7").as_deref(),
            Some("203.0.113.7")
        );
        assert_eq!(
            valid_public_ip("  203.0.113.7\n").as_deref(),
            Some("203.0.113.7")
        );
    }

    #[test]
    fn valid_public_ip_accepts_ipv6() {
        assert!(valid_public_ip("2001:db8::1").is_some());
    }

    #[test]
    fn valid_public_ip_rejects_non_ip_bodies() {
        // An error page / HTML / rate-limit text must not leak into the UI.
        assert_eq!(valid_public_ip("<html>error</html>"), None);
        assert_eq!(valid_public_ip(""), None);
        assert_eq!(valid_public_ip("rate limited"), None);
        assert_eq!(valid_public_ip("999.999.999.999"), None);
    }

    #[test]
    fn newest_crash_text_gates_out_stale_reports() {
        use std::time::{Duration, SystemTime};
        let dir = tempfile::tempdir().unwrap();
        let crash = dir.path().join("crash-2026-06-19_18.16.56-server.txt");
        std::fs::write(&crash, "Description: Exception in server tick loop\n").unwrap();

        // Anchor in the FUTURE relative to the file → it reads as stale → ignored.
        let future = SystemTime::now() + Duration::from_secs(3600);
        assert_eq!(
            newest_crash_text(dir.path(), 1024, Some(future)),
            None,
            "a crash report older than this run's start must be ignored"
        );

        // Anchor at the epoch → the file is newer than the anchor → considered.
        assert!(
            newest_crash_text(dir.path(), 1024, Some(SystemTime::UNIX_EPOCH)).is_some(),
            "a same-run (newer-than-anchor) crash report must be read"
        );

        // No anchor → freshness gate skipped (back-compat) → considered.
        assert!(newest_crash_text(dir.path(), 1024, None).is_some());
    }

    #[test]
    fn newest_crash_text_none_for_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-crash-reports");
        assert_eq!(newest_crash_text(&missing, 1024, None), None);
    }
}

#[cfg(test)]
mod password_ux_tests {
    use super::resolve_upload_secret;
    use crate::servers_runtime::transfer::UploadAuthMethod;

    #[test]
    fn transient_password_is_used_in_place_of_keyring() {
        let got = resolve_upload_secret(
            UploadAuthMethod::Password,
            Some("typed-now".to_string()),
            Some("from-keyring".to_string()),
        )
        .unwrap();
        assert_eq!(got, "typed-now");
    }

    #[test]
    fn falls_back_to_keyring_when_no_transient() {
        let got = resolve_upload_secret(
            UploadAuthMethod::Password,
            None,
            Some("from-keyring".to_string()),
        )
        .unwrap();
        assert_eq!(got, "from-keyring");
    }

    #[test]
    fn password_auth_with_no_secret_anywhere_is_not_configured() {
        let r = resolve_upload_secret(UploadAuthMethod::Password, None, None);
        assert!(matches!(r, Err(crate::error::Error::UploadNotConfigured)));
    }

    #[test]
    fn key_auth_with_no_secret_yields_empty_passphrase() {
        let got = resolve_upload_secret(UploadAuthMethod::Key, None, None).unwrap();
        assert_eq!(got, "");
    }

    #[test]
    fn empty_transient_string_still_counts_as_provided() {
        let got = resolve_upload_secret(
            UploadAuthMethod::Key,
            Some(String::new()),
            Some("kr".into()),
        )
        .unwrap();
        assert_eq!(got, "");
    }
}
