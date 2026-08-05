//! Tauri-команды фичи «Свой сервер» (План 1: создание/список/удаление).

use crate::error::{Error, Result};
use crate::mods::platform::{InstalledMod, ModVersion, ServerSideSupport};
use crate::mods::updates::{classify_update, ModUpdateCheck, ModUpdateState};
use crate::servers_runtime::schema::{ServerCore, ServerFile, ServerWithStatus, UploadConfig};
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

/// `ServerModEntry` + the install-identity overlay (sha1-keyed registry).
/// Identity fields are `Option`: locally-dropped jars carry no record until
/// enriched. `name`/`version_number` are hints; the UI resolves the display
/// name from the platform by `project_id`.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerModEntryEnriched {
    pub filename: String,
    /// Current on-disk name (`filename` + `.disabled` when disabled). Mutations
    /// (delete/enable/disable) MUST join this, not the base `filename`.
    pub on_disk_filename: String,
    pub disabled: bool,
    pub reason: Option<String>,
    pub sha1: String,
    pub source: Option<crate::mods::platform::ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version_number: Option<String>,
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
    let mut out: HashMap<String, ServerSideSupport> = HashMap::new();
    // Whole-jar reads + SHA-1 on a blocking thread (same rationale as
    // `enrich_server_dir`): a modded server's `mods/` is gigabytes.
    let sha_by_file: HashMap<String, String> = {
        let dir = mods_dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            use sha1::{Digest, Sha1};
            let mut sha_by_file: HashMap<String, String> = HashMap::new();
            let Ok(rd) = std::fs::read_dir(&dir) else {
                return sha_by_file;
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
            sha_by_file
        })
        .await
        .unwrap_or_default()
    };
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

/// Build a `ServerWithStatus` from the file + live runtime status (running/
/// pid/port) + the keyring password-presence flag. Single source for list/
/// rename/update so the status-enrichment logic isn't duplicated.
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
            last_exit_code,
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
    loader: ServerCore,
    loader_version: Option<String>,
    max_heap_mb: u32,
    eula_accepted: bool,
    created_from_instance: Option<String>,
) -> Result<ServerCreated> {
    crate::data_root::reject_if_fallen_back(&app)?;
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    // Trim + reject empty/duplicate names at the boundary (the wizard also gates
    // this, but two concurrent creates could still collide on the same name).
    let name = store::validate_name(&name, &store::list_all(&base)?, None)?;
    // Reserve a readable, unique directory SYNCHRONOUSLY before the async
    // provisioning below. validate_name is not a reservation: two distinct
    // names can slugify to the same directory, and the async gap would let two
    // concurrent creates race into one directory. The reserved name is the id.
    let servers_parent =
        crate::paths::servers_dir(&app).map_err(|e| Error::io("<servers_dir>", e))?;
    let (id, reserved_dir) =
        crate::naming::reserve_unique_dir(&servers_parent, &name, None, "server")?;
    // Remove the reserved directory if any step below fails (`?`), so a partial
    // create never leaks the slug (forcing every future same-name create to -2).
    // Disarmed on success via `keep()`.
    let cleanup = crate::naming::DirCleanup::new(&reserved_dir);

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
    provision_loader(&app, &base, &mut file).await?;
    let mut quarantined: Vec<String> = Vec::new();
    if let Some(inst_id) = &file.created_from_instance {
        let src = crate::paths::mods_dir(&app, inst_id)
            .map_err(|e| crate::error::Error::io("<instance_mods_dir>", e))?;
        let dest = crate::paths::server_paths(&base, &file.id).mods;
        let copied = crate::servers_runtime::create::copy_instance_mods(&src, &dest)?;
        crate::diag!("servers: copied {copied} mods from instance {inst_id}");
        // Proactively set aside client-only mods so a modpack server can start
        // instead of crashing one client mod at a time. Best-effort — never
        // fails creation; a metadata miss degrades to offline detection.
        let side_map = server_side_by_instance_mods(&app, inst_id).await;
        match crate::servers_runtime::quarantine::quarantine_with_metadata(&dest, &side_map) {
            Ok((disabled, _)) => {
                if !disabled.is_empty() {
                    crate::diag!("servers: quarantined {} client mods", disabled.len());
                }
                quarantined = disabled;
            }
            Err(e) => crate::diag!("servers: client-mod quarantine skipped: {e}"),
        }
    }
    cleanup.keep();
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
    crate::data_root::reject_if_fallen_back(&app)?;
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

/// Принудительно завершить сервер СЕЙЧАС: force-kill без graceful-ожидания.
/// Эскалация из идущего `server_stop`, когда сервер завис/ещё грузится и не
/// обрабатывает `stop`. Пишет код выхода `-1` (наш sentinel), так что это
/// «Остановлен», а не «Аварийно завершён».
#[tauri::command]
#[specta::specta]
pub fn server_kill(app: AppHandle, id: String) -> Result<()> {
    crate::servers_runtime::runtime::kill(&app, &id)
}

/// Перезапустить сервер (stop если запущен, затем start).
#[tauri::command]
#[specta::specta]
pub async fn server_restart(app: AppHandle, id: String) -> Result<u32> {
    crate::data_root::reject_if_fallen_back(&app)?;
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

/// Like `server_list_mods`, but each jar carries its registry identity. Uses
/// `reconcile_on_list` (sha1-keyed) so identity survives enable/disable renames.
#[tauri::command]
#[specta::specta]
pub fn server_list_mods_enriched(
    app: AppHandle,
    id: String,
) -> Result<Vec<ServerModEntryEnriched>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let reasons = crate::servers_runtime::quarantine::read_reasons(&mods);
    let entries = crate::servers_runtime::installed::reconcile_on_list(&mods)?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let disabled = !e.enabled;
            // Current on-disk name: base `filename` + `.disabled` when disabled.
            // Used both for the quarantine-reason lookup and as `on_disk_filename`
            // so mutation commands join the real file, not the base name.
            let on_disk = if disabled {
                format!("{}.disabled", e.record.filename)
            } else {
                e.record.filename.clone()
            };
            let reason = if disabled {
                reasons.get(&on_disk).cloned()
            } else {
                None
            };
            ServerModEntryEnriched {
                filename: e.record.filename,
                on_disk_filename: on_disk,
                disabled,
                reason,
                sha1: e.record.sha1,
                source: e.record.source,
                project_id: e.record.project_id,
                version_id: e.record.version_id,
                name: e.record.name,
                version_number: e.record.version_number,
            }
        })
        .collect())
}

/// Pure resolve + tie-break + one-shot gate for a scope of lowercased sha1s.
/// `home = Modrinth` tie-break: a sha present in both platform maps resolves to
/// Modrinth. The gate marks a sha `attempted` only when every *tried* platform
/// returned OK (a partial failure leaves the whole scope un-attempted so the
/// next pass retries), while still surfacing any identities that DID resolve.
/// Extracted from `enrich_server_dir` so the branchy logic is unit-testable
/// without a network or filesystem.
fn resolve_and_gate(
    scope_shas: &[String],
    mr_hits: &std::collections::HashMap<String, crate::mods::modrinth::HashVersion>,
    mr_ok: bool,
    cf_hits: &std::collections::HashMap<String, crate::mods::curseforge::FingerprintFile>,
    cf_ok: bool,
    cf_tried: bool,
) -> (
    std::collections::HashMap<String, crate::servers_runtime::installed::ResolvedServerIdentity>,
    std::collections::HashSet<String>,
) {
    use crate::servers_runtime::installed::ResolvedServerIdentity;
    let mut resolved = std::collections::HashMap::new();
    for sha in scope_shas {
        let id = match (mr_hits.get(sha), cf_hits.get(sha)) {
            (Some(mr), _) => ResolvedServerIdentity {
                source: crate::mods::platform::ModSource::Modrinth,
                project_id: mr.project_id.clone(),
                version_id: Some(mr.version_id.clone()),
                name: Some(mr.name.clone()),
                version_number: Some(mr.version_number.clone()),
            },
            (None, Some(cf)) => ResolvedServerIdentity {
                source: crate::mods::platform::ModSource::Curseforge,
                project_id: cf.project_id.clone(),
                version_id: Some(cf.version_id.clone()),
                name: None,
                version_number: cf.version_number.clone(),
            },
            (None, None) => continue,
        };
        resolved.insert(sha.clone(), id);
    }
    let all_tried_succeeded = mr_ok && (!cf_tried || cf_ok);
    let attempted = if all_tried_succeeded {
        scope_shas.iter().cloned().collect()
    } else {
        std::collections::HashSet::new()
    };
    (resolved, attempted)
}

/// Hash-enrich untracked jars in a server dir, mirroring
/// `mods::enrich::enrich_untracked`: SHA-1 → Modrinth (`versions_by_hashes`)
/// for both dirs; Murmur2 → CurseForge (`files_by_fingerprint`) for mods only.
/// `home = Modrinth` tie-break. Best-effort: a failure degrades to "no match"
/// and does NOT mark the jar attempted (so the next pass retries).
///
/// Hangar has NO hash-lookup endpoint, so plugins resolve via Modrinth's
/// plugin index only; unresolved plugins keep `source = None` but are still
/// marked attempted so they aren't re-queried every pass.
///
/// The blocking work (directory scan + sha1/Murmur2 hashing, and the final
/// sidecar write) runs on `spawn_blocking` threads so the async executor is
/// never blocked on disk I/O; the two network calls stay on the async task.
async fn enrich_server_dir(dir: &std::path::Path, use_cf: bool) -> Result<u32> {
    use std::collections::HashMap;

    // Blocking phase 1: reconcile the sidecar against disk, filter to untracked
    // jars, and (for the CF path) fingerprint each jar's bytes. Runs off the
    // async executor since it is pure disk I/O + hashing.
    let scan_dir = dir.to_path_buf();
    let (scope_shas, fingerprints): (Vec<String>, Vec<(u32, String)>) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<String>, Vec<(u32, String)>)> {
            let entries = crate::servers_runtime::installed::reconcile_on_list(&scan_dir)?;
            let in_scope: Vec<_> = entries
                .into_iter()
                .filter(|e| e.record.source.is_none() && !e.record.enrich_attempted)
                .collect();
            let mut shas: Vec<String> = Vec::new();
            let mut fingerprints: Vec<(u32, String)> = Vec::new();
            for e in &in_scope {
                // Enabled jars are `*.jar`; set-aside jars are `*.jar.disabled`.
                let on_disk = if e.enabled {
                    e.record.filename.clone()
                } else {
                    format!("{}.disabled", e.record.filename)
                };
                let sha = e.record.sha1.to_ascii_lowercase();
                shas.push(sha.clone());
                if use_cf {
                    // Already on a blocking thread → sync read is correct here.
                    if let Ok(bytes) = std::fs::read(scan_dir.join(&on_disk)) {
                        fingerprints
                            .push((crate::mods::enrich::curseforge_fingerprint(&bytes), sha));
                    }
                }
            }
            Ok((shas, fingerprints))
        })
        .await
        .map_err(|e| Error::io("<enrich-scan>", e))??;

    if scope_shas.is_empty() {
        return Ok(0);
    }

    let mr = crate::mods::modrinth::ModrinthClient::new();
    let shas_ref: Vec<&str> = scope_shas.iter().map(String::as_str).collect();
    let (mr_hits, mr_ok) = match mr.versions_by_hashes(&shas_ref).await {
        Ok(m) => (m, true),
        Err(_) => (HashMap::new(), false),
    };

    let cf_tried = use_cf;
    let (cf_hits, cf_ok) = if use_cf {
        let cf = crate::mods::curseforge::CurseForgeClient::new();
        match cf.files_by_fingerprint(&fingerprints).await {
            Ok(m) => (m, true),
            Err(_) => (HashMap::new(), false),
        }
    } else {
        (HashMap::new(), true) // not tried => vacuous success
    };

    let (resolved, attempted) =
        resolve_and_gate(&scope_shas, &mr_hits, mr_ok, &cf_hits, cf_ok, cf_tried);

    let n = resolved.len() as u32;
    // Blocking phase 2: persist the identities + attempted flags off the executor.
    let apply_dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        crate::servers_runtime::installed::apply_enrichment(&apply_dir, &resolved, &attempted)
    })
    .await
    .map_err(|e| Error::io("<enrich-apply>", e))??;
    Ok(n)
}

/// Hash-enrich a server's `runtime/mods/` (Modrinth + CurseForge). Returns the
/// count newly resolved. Best-effort — never blocks the UI.
#[tauri::command]
#[specta::specta]
pub async fn server_enrich_mods(app: AppHandle, id: String) -> Result<u32> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let use_cf = crate::mods::curseforge::keyring::resolve().is_some();
    enrich_server_dir(&p.mods, use_cf).await
}

/// Hash-enrich a server's `runtime/plugins/` via Modrinth only (Hangar has no
/// hash endpoint; CurseForge has no plugin registry).
#[tauri::command]
#[specta::specta]
pub async fn server_enrich_plugins(app: AppHandle, id: String) -> Result<u32> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    enrich_server_dir(&p.plugins, /* use_cf = */ false).await
}

/// Удалить мод из папки `mods/` сервера по имени файла.
/// Идемпотентно: файл уже удалён → `Ok`.
/// Отклоняет небезопасные имена (path traversal).
#[tauri::command]
#[specta::specta]
pub fn server_delete_mod(app: AppHandle, id: String, filename: String) -> Result<()> {
    // Match server_delete_plugin: never delete a jar out from under a running
    // server. Closes the gap the plugin twin's doc comment previously flagged.
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(crate::error::Error::ServerAlreadyRunning { id });
    }
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(crate::error::Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let path = mods.join(&filename);
    if !path.starts_with(&mods) {
        return Err(crate::error::Error::server_file_invalid(
            filename.as_str(),
            "path escapes mods dir",
        ));
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
        classify_client_only_mods, diagnose_server_run, dist_crash_tokens, extract_missing_dep_ids,
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
    // Gate the log verdict on the recorded exit code: Forge's `invalid dist
    // DEDICATED_SERVER` warning is non-fatal, so a server the user stopped before
    // it finished loading would otherwise mis-report as a client-mod crash. A
    // clean (0) or force-killed (-1) exit proves it was stopped, not crashed.
    let exit_code = crate::servers_runtime::exit_state::read(&p.runtime);
    let diagnosis = diagnose_server_run(diag_input, exit_code);

    // Reads + zip-parses every jar in `mods/` — blocking disk/CPU work, so run
    // the pass on a blocking thread (same rationale as `enrich_server_dir`).
    let mods: Vec<(String, crate::mods::local::ModEnvironment)> = {
        let dir = p.mods.clone();
        tokio::task::spawn_blocking(move || {
            let mut mods = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&dir) {
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
            mods
        })
        .await
        .unwrap_or_default()
    };
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
        if let Some(code) = exit_code {
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
            return Err(Error::server_file_invalid(f.as_str(), "invalid filename"));
        }
        let path = p.mods.join(f);
        if !path.starts_with(&p.mods) {
            return Err(Error::server_file_invalid(
                f.as_str(),
                "path escapes mods dir",
            ));
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
    let mut file = crate::servers_runtime::store::read_server_json(&p.json)?;
    provision_loader(&app, &base, &mut file).await?;
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
            return Err(Error::server_file_invalid(f.as_str(), "invalid filename"));
        }
        let src = p.mods.join(f);
        if !src.starts_with(&p.mods) {
            return Err(Error::server_file_invalid(
                f.as_str(),
                "path escapes mods dir",
            ));
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

/// The client `LoaderKind` the server's Java-mod machinery should use, or a
/// fast typed error when this core has none. Contract: only mod-capable cores
/// (Fabric/Quilt/Forge/NeoForge) pass; vanilla and the Bukkit plugin cores
/// (Paper/Purpur) are rejected rather than silently installing into a `mods/`
/// dir the server never reads (matches the UI's per-core gating).
fn require_mod_loader(file: &ServerFile) -> Result<crate::instances::schema::LoaderKind> {
    file.loader
        .as_loader_kind()
        .filter(|_| file.loader.mod_capable())
        .ok_or_else(|| Error::ServerCoreUnsupported {
            reason: "this server core does not load mods".into(),
        })
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
    let loader = require_mod_loader(&file)?;
    let cf_key = crate::mods::curseforge::keyring::resolve();
    let report = crate::mods::dep_resolve::install_missing_into_dir(
        &base,
        &p.mods,
        &mod_ids,
        &file.mc_version,
        loader,
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
    crate::data_root::reject_if_fallen_back(&app)?;
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
    file: &mut ServerFile,
) -> Result<()> {
    match file.loader {
        ServerCore::Vanilla => {
            let (jar_url, sha1) = create::resolve_vanilla_jar(&file.mc_version).await?;
            create::create_vanilla_server(base, file, &jar_url, &sha1).await?;
        }
        ServerCore::Fabric => {
            let installer = create::latest_fabric_installer(&file.mc_version).await?;
            let lv = create::require_loader_version(file, "fabric")?;
            let url = crate::servers_runtime::jar::fabric_server_jar_url(
                &file.mc_version,
                &lv,
                &installer,
            );
            create::create_fabric_server(base, file, &url).await?;
        }
        ServerCore::Quilt => {
            let installer = create::latest_quilt_installer(&file.mc_version).await?;
            let lv = create::require_loader_version(file, "quilt")?;
            let url = crate::servers_runtime::jar::quilt_server_jar_url(
                &file.mc_version,
                &lv,
                &installer,
            );
            create::create_quilt_server(base, file, &url).await?;
        }
        ServerCore::Forge | ServerCore::NeoForge => {
            let lv = create::require_loader_version(file, "forge/neoforge")?;
            let (flavor, label) = if matches!(file.loader, ServerCore::Forge) {
                (crate::forge::ForgeFlavor::Forge, "forge")
            } else {
                (crate::forge::ForgeFlavor::NeoForge, "neoforge")
            };
            // Same fetch as the client instance path: maven `.sha1` sidecar
            // verification + shared on-disk installer cache.
            let bytes =
                crate::forge::meta::fetch_installer_bytes(flavor, &file.mc_version, &lv, app)
                    .await?;
            let component = create::resolve_server_java_component(&file.mc_version).await?;
            crate::jre::ensure_jre(&component, app, |_, _, _| {}).await?;
            let java_bin = crate::jre::java_executable_path(&component, app)?;
            create::create_installer_server(base, file, &bytes, &java_bin, label).await?;
        }
        ServerCore::Paper => {
            let jar = crate::servers_runtime::paper::PaperClient::new()
                .latest_stable_build(&file.mc_version)
                .await?;
            // Persist the resolved build BEFORE the prebuilt path writes
            // server.json, so the stored loader_version is the real build.
            file.loader_version = Some(jar.build.clone());
            create::create_paper_family_server(base, file, &jar.url, jar.checksum).await?;
        }
        ServerCore::Purpur => {
            let jar = crate::servers_runtime::purpur::PurpurClient::new()
                .latest_successful_build(&file.mc_version)
                .await?;
            file.loader_version = Some(jar.build.clone());
            create::create_paper_family_server(base, file, &jar.url, jar.checksum).await?;
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
    loader: ServerCore,
    loader_version: Option<String>,
    max_heap_mb: u32,
    eula_accepted: bool,
) -> Result<ServerWithStatus> {
    crate::data_root::reject_if_fallen_back(&app)?;
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
        //
        // Reserve the readable directory synchronously up front (the reserved
        // name is the id); any failure below removes it so a partial import
        // never leaks the slug.
        let servers_parent =
            crate::paths::servers_dir(&app).map_err(|e| Error::io("<servers_dir>", e))?;
        let (new_id, reserved_dir) =
            crate::naming::reserve_unique_dir(&servers_parent, &name, None, "server")?;
        // Remove the reserved directory if any step below fails (`?` / early
        // return), so a partial import never leaks the slug.
        let cleanup = crate::naming::DirCleanup::new(&reserved_dir);
        let mut file = import::build_file(
            &new_id,
            &name,
            &mc_version,
            loader,
            loader_version,
            max_heap_mb,
            eula_accepted,
        );
        provision_loader(&app, &base, &mut file).await?;
        let p = crate::paths::server_paths(&base, &new_id);
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
        cleanup.keep();
        new_id
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
        return Err(Error::server_file_invalid(
            file_name.as_str(),
            "invalid log filename",
        ));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let path = crate::paths::server_paths(&base, &id).logs.join(&file_name);
    // A log the server hasn't produced yet is "empty", not an error (the
    // console backfill reads server-latest.log right after first start).
    if !path.exists() {
        return Ok(String::new());
    }
    // But a real read failure (permission, lock) propagates instead of
    // rendering as an empty log — sibling commands in `commands::logs` do too.
    crate::logs::read::read_with_cap(&path, 1024 * 1024)
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
/// Console markers a server prints when `save-all flush` completes. Modern
/// vanilla/Paper log "Saved the game"; pre-1.13 era logs "Saved the world".
/// Matched case-insensitively as substrings of the console line.
const SAVE_CONFIRMATION_MARKERS: [&str; 2] = ["saved the game", "saved the world"];

/// True iff this console line confirms a completed `save-all flush`.
fn is_save_confirmation(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    SAVE_CONFIRMATION_MARKERS.iter().any(|m| l.contains(m))
}

/// Upper bound on waiting for the save confirmation. A huge modded world can
/// flush for a while; past this we proceed (having waited far longer than the
/// old fixed 800 ms guess) rather than hang the backup.
const SAVE_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Flush + pause world saves on a RUNNING server before zipping — the dance
/// shared by the manual backup and the auto-backup scheduler. `save-all flush`
/// is asynchronous (the server confirms with a console line), so subscribe to
/// console output and await the marker instead of sleeping a guessed interval.
/// Best-effort by design (the backup itself must still happen), but every
/// failure is recorded via `diag!` — a torn or autosave-live snapshot should
/// never be silent.
async fn pause_saves_for_backup(id: &str) {
    use crate::servers_runtime::runtime;
    // Subscribe BEFORE sending the command so the confirmation can't slip
    // through between send and subscribe.
    let mut rx = runtime::subscribe_lines(id);
    match runtime::send_command(id, "save-all flush").await {
        Err(e) => crate::diag!("server backup: {id}: save-all flush failed: {e}"),
        Ok(()) => {
            let confirmed = tokio::time::timeout(SAVE_FLUSH_TIMEOUT, async {
                loop {
                    match rx.recv().await {
                        Ok(line) if is_save_confirmation(&line) => return true,
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
                    }
                }
            })
            .await;
            match confirmed {
                Ok(true) => {}
                Ok(false) => crate::diag!(
                    "server backup: {id}: console closed before save confirmation \
                     (server stopping?) — proceeding"
                ),
                Err(_) => crate::diag!(
                    "server backup: {id}: no save confirmation within {}s — proceeding",
                    SAVE_FLUSH_TIMEOUT.as_secs()
                ),
            }
        }
    }
    if let Err(e) = runtime::send_command(id, "save-off").await {
        crate::diag!("server backup: {id}: save-off failed — snapshot may be torn: {e}");
    }
}

/// Re-enable autosave after a hot backup. Failure is diag-logged: a server left
/// with autosave off keeps running but stops persisting the world.
async fn resume_saves_after_backup(id: &str) {
    if let Err(e) = crate::servers_runtime::runtime::send_command(id, "save-on").await {
        crate::diag!("server backup: {id}: save-on failed — autosave may stay off: {e}");
    }
}

#[tauri::command]
#[specta::specta]
pub async fn server_backup_create(app: AppHandle, id: String) -> Result<backup::BackupInfo> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let running = crate::servers_runtime::runtime::is_running(&id);
    if running {
        pause_saves_for_backup(&id).await;
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    // Sync walk + zip of a potentially GB-scale runtime — off the async runtime.
    let res = {
        let base = base.clone();
        let id_task = id.clone();
        tokio::task::spawn_blocking(move || backup::create_backup(&base, &id_task, &stamp))
            .await
            .map_err(|e| Error::io("<server_backup_create>", format!("join: {e}")))?
    };
    if running {
        resume_saves_after_backup(&id).await;
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
    // Sync zip + tree replace of a potentially GB-scale runtime — off the
    // async runtime (same as server_backup_create).
    tokio::task::spawn_blocking(move || {
        backup::create_backup_protecting(&base, &id, &stamp, Some(&file_name))?;
        backup::restore_backup(&base, &id, &file_name)
    })
    .await
    .map_err(|e| Error::io("<server_backup_restore>", format!("join: {e}")))?
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
            pause_saves_for_backup(&id).await;
            let res = {
                let base = base.clone();
                let id_task = id.clone();
                match tokio::task::spawn_blocking(move || {
                    crate::servers_runtime::backup::maybe_auto_backup(&base, &id_task, now, &stamp)
                })
                .await
                {
                    Ok(r) => r,
                    Err(e) => Err(Error::io("<auto-backup>", format!("join: {e}"))),
                }
            };
            resume_saves_after_backup(&id).await;
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
    let loader = require_mod_loader(&file)?;
    let report = crate::commands::install_version_into_dir(
        &base,
        &p.mods,
        source,
        &project_id,
        &version_id,
        &file.mc_version,
        loader,
    )
    .await?;

    // Record the user-picked primary's identity so the enriched list can show it.
    // The mods kernel pushes the primary LAST (deps first). Best-effort AND off the
    // async executor: `sha1_of` does a full-file jar read and `upsert` writes the
    // sidecar — both blocking, so run them in `spawn_blocking` (mirrors
    // `install_local_plugin`). A sidecar failure must never fail an install already
    // completed on disk.
    // Fast-follow: `copy_version_into_dir` already verifies this sha1 during the
    // download; a future change could thread it out of `InstallMissingReport` to
    // skip this re-read (deferred — that struct is shared with the client path).
    if let Some(primary) = report.installed.last() {
        let dir = p.mods.clone();
        let primary = primary.clone();
        let (src, pid, vid) = (source, project_id, version_id);
        let _ = tokio::task::spawn_blocking(move || {
            let jar = dir.join(&primary);
            if let Ok(sha1) = crate::servers_runtime::installed::sha1_of(&jar) {
                let _ = crate::servers_runtime::installed::upsert(
                    &dir,
                    crate::servers_runtime::installed::ServerInstalledRecord {
                        filename: primary,
                        sha1: sha1.to_ascii_lowercase(),
                        source: Some(src),
                        project_id: Some(pid),
                        version_id: Some(vid),
                        name: None,
                        version_number: None,
                        enrich_attempted: false,
                    },
                );
            }
        })
        .await;
    }
    Ok(report)
}

/// Adapt a registry record (identity-bearing) into the `InstalledMod` shape
/// `classify_update` consumes. `classify_update` reads only `version_id`, but
/// map faithfully so it stays correct if the classifier widens.
fn record_as_installed(
    r: &crate::servers_runtime::installed::ServerInstalledRecord,
    enabled: bool,
) -> InstalledMod {
    InstalledMod {
        filename: r.filename.clone(),
        sha1: r.sha1.clone(),
        source: r.source,
        project_id: r.project_id.clone(),
        version_id: r.version_id.clone(),
        name: r.name.clone().unwrap_or_else(|| r.filename.clone()),
        version_number: r.version_number.clone(),
        installed_at: String::new(),
        enabled,
        enrich_attempted: r.enrich_attempted,
        requires: Vec::new(),
    }
}

/// Check every identity-bearing server mod for a newer version. Mirrors
/// `mods_check_updates`: resolve mc_version + loader from `server.json`, query
/// each mod's platform, classify with the shared `classify_update`. Per-mod
/// failure → that row's `CheckFailed`.
#[tauri::command]
#[specta::specta]
pub async fn server_check_mod_updates(app: AppHandle, id: String) -> Result<Vec<ModUpdateCheck>> {
    use futures_util::stream::{self, StreamExt};

    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let loader = require_mod_loader(&file)?; // rejects vanilla/plugin cores pre-network
    let mc_version = file.mc_version;

    // Reconcile the sidecar against disk off the async executor: it does a full
    // byte read + Sha1 of every jar (mirrors `enrich_server_dir`).
    let entries = {
        let dir = p.mods.clone();
        tokio::task::spawn_blocking(move || {
            crate::servers_runtime::installed::reconcile_on_list(&dir)
        })
        .await
        .map_err(|e| Error::io("<reconcile>", e))??
    };

    // Only records with full platform identity are checkable. Enumerate first
    // (mirrors the client `mods_check_updates`): the paired index restores the
    // installed order after the unordered concurrent poll.
    let eligible: Vec<(
        usize,
        InstalledMod,
        crate::mods::platform::ModSource,
        String,
    )> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            match (
                e.record.source,
                e.record.project_id.clone(),
                e.record.version_id.clone(),
            ) {
                (Some(source), Some(pid), Some(_vid)) => {
                    Some((i, record_as_installed(&e.record, e.enabled), source, pid))
                }
                _ => None,
            }
        })
        .collect();

    const CONCURRENCY: usize = 6;
    let mut results: Vec<(usize, ModUpdateCheck)> = stream::iter(eligible)
        .map(|(i, m, source, project_id)| {
            let mc = mc_version.clone();
            async move {
                let platform = super::platform_for(source);
                let state = match platform
                    .versions(&project_id, Some(&mc), Some(loader))
                    .await
                {
                    Ok(versions) => classify_update(&m, &versions),
                    Err(e) => ModUpdateState::CheckFailed {
                        reason: e.to_string(),
                    },
                };
                (
                    i,
                    ModUpdateCheck {
                        sha1: m.sha1.clone(),
                        name: m.name.clone(),
                        source,
                        project_id,
                        current_version_id: m.version_id.clone().unwrap_or_default(),
                        current_version_number: m.version_number.clone(),
                        state,
                    },
                )
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    results.sort_by_key(|(i, _)| *i);
    Ok(results.into_iter().map(|(_, c)| c).collect())
}

/// Check every identity-bearing server plugin for a newer version. The plugin
/// twin of [`server_check_mod_updates`]: gate on the core being plugin-capable
/// (Paper/Purpur) before any network, reconcile `runtime/plugins/`, query each
/// plugin's platform via `plugin_versions` (plugin-loader slug lineage, not a
/// `LoaderKind`), and classify with the shared `classify_update`. Per-plugin
/// failure → that row's `CheckFailed`.
#[tauri::command]
#[specta::specta]
pub async fn server_check_plugin_updates(
    app: AppHandle,
    id: String,
) -> Result<Vec<ModUpdateCheck>> {
    use futures_util::stream::{self, StreamExt};

    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    // Reject vanilla / mod cores before any network — plugins only load on
    // Bukkit-family cores (Paper/Purpur).
    if !file.loader.plugin_capable() {
        return Err(Error::ServerCoreUnsupported {
            reason: "this server core does not load plugins".into(),
        });
    }
    let core = file.loader;
    let mc_version = file.mc_version;

    // Reconcile the sidecar against disk off the async executor (full byte read
    // + Sha1 of every jar, mirrors `enrich_server_dir`).
    let entries = {
        let dir = p.plugins.clone();
        tokio::task::spawn_blocking(move || {
            crate::servers_runtime::installed::reconcile_on_list(&dir)
        })
        .await
        .map_err(|e| Error::io("<reconcile>", e))??
    };

    // Only records with full platform identity are checkable. Enumerate first so
    // the paired index restores installed order after the unordered poll.
    let eligible: Vec<(
        usize,
        InstalledMod,
        crate::mods::platform::ModSource,
        String,
    )> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            match (
                e.record.source,
                e.record.project_id.clone(),
                e.record.version_id.clone(),
            ) {
                (Some(source), Some(pid), Some(_vid)) => {
                    Some((i, record_as_installed(&e.record, e.enabled), source, pid))
                }
                _ => None,
            }
        })
        .collect();

    const CONCURRENCY: usize = 6;
    let mut results: Vec<(usize, ModUpdateCheck)> = stream::iter(eligible)
        .map(|(i, m, source, project_id)| {
            let mc = mc_version.clone();
            async move {
                let platform = super::platform_for(source);
                let state = match platform
                    .plugin_versions(&project_id, Some(&mc), core.plugin_loader_slugs())
                    .await
                {
                    Ok(versions) => classify_update(&m, &versions),
                    Err(e) => ModUpdateState::CheckFailed {
                        reason: e.to_string(),
                    },
                };
                (
                    i,
                    ModUpdateCheck {
                        sha1: m.sha1.clone(),
                        name: m.name.clone(),
                        source,
                        project_id,
                        current_version_id: m.version_id.clone().unwrap_or_default(),
                        current_version_number: m.version_number.clone(),
                        state,
                    },
                )
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    results.sort_by_key(|(i, _)| *i);
    Ok(results.into_iter().map(|(_, c)| c).collect())
}

/// Idempotent, path-guarded removal of one file under a server's `mods/`.
fn remove_server_mod_file(mods_dir: &std::path::Path, on_disk_name: &str) -> Result<()> {
    if !crate::servers_runtime::runtime::is_safe_mod_name(on_disk_name) {
        return Err(Error::server_file_invalid(on_disk_name, "invalid filename"));
    }
    let path = mods_dir.join(on_disk_name);
    if !path.starts_with(mods_dir) {
        return Err(Error::server_file_invalid(
            on_disk_name,
            "path escapes mods dir",
        ));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

/// Decide the file/registry swap for an update. `new_name` is the actual
/// installed primary's on-disk name (always enabled `<base>.jar`). Returns
/// (old on-disk file to delete or None if it's the same file we just wrote,
///  whether to re-disable the new primary, the new registry record).
fn plan_swap(
    old_enabled: bool,
    old_base_filename: &str,
    new_name: &str,
    new_sha1: &str,
    target: &ModVersion,
) -> (
    Option<String>,
    bool,
    crate::servers_runtime::installed::ServerInstalledRecord,
) {
    let old_on_disk = if old_enabled {
        old_base_filename.to_string()
    } else {
        format!("{old_base_filename}.disabled")
    };
    let delete_old = if old_on_disk != new_name {
        Some(old_on_disk)
    } else {
        None
    };
    let record = crate::servers_runtime::installed::ServerInstalledRecord {
        filename: new_name.to_string(),
        sha1: new_sha1.to_ascii_lowercase(),
        source: Some(target.source),
        project_id: Some(target.project_id.clone()),
        version_id: Some(target.version_id.clone()),
        name: Some(target.name.clone()),
        version_number: Some(target.version_number.clone()),
        enrich_attempted: true,
    };
    (delete_old, !old_enabled, record)
}

/// Apply one server-mod update: install `target` (+ required deps) via the
/// shared kernel, remove the old jar (honoring its `.disabled` suffix), preserve
/// set-aside state, and swap the registry rows. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_update_one(
    app: AppHandle,
    id: String,
    old_sha1: String,
    target: ModVersion,
) -> Result<crate::mods::dep_resolve::InstallMissingReport> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let loader = require_mod_loader(&file)?;

    // Reconcile the sidecar against disk off the async executor: it does a full
    // byte read + Sha1 of every jar (mirrors `enrich_server_dir`).
    let entries = {
        let dir = p.mods.clone();
        tokio::task::spawn_blocking(move || {
            crate::servers_runtime::installed::reconcile_on_list(&dir)
        })
        .await
        .map_err(|e| Error::io("<reconcile>", e))??
    };

    let old = entries
        .into_iter()
        .find(|e| e.record.sha1.eq_ignore_ascii_case(&old_sha1))
        .ok_or_else(|| Error::ServerContentStale)?;

    let report = crate::commands::install_version_into_dir(
        &base,
        &p.mods,
        target.source,
        &target.project_id,
        &target.version_id,
        &file.mc_version,
        loader,
    )
    .await?;

    // Derive the swap from the ACTUAL installed primary, never the caller's
    // `target`: `install_version_into_dir` re-resolves the version server-side
    // and writes whatever THAT returns, so the client-echoed filename/sha1 can
    // differ (targeting the wrong file → a silently-swallowed NotFound → a
    // set-aside mod coming back enabled, or a registry row that doesn't match
    // disk). The mods kernel pushes the primary LAST (deps first).
    let Some(new_name) = report.installed.last().cloned() else {
        // Install succeeded but reported no primary — nothing to reconcile.
        return Ok(report);
    };
    // File + registry bookkeeping off the async executor (jar hash + fs + sidecar).
    let mods = p.mods.clone();
    let old_enabled = old.enabled;
    let old_base = old.record.filename.clone();
    let old_record_sha1 = old.record.sha1.clone();
    let target_for_record = target.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let new_sha1 = crate::servers_runtime::installed::sha1_of(&mods.join(&new_name))?;
        let (delete_old, redisable, new_record) = plan_swap(
            old_enabled,
            &old_base,
            &new_name,
            &new_sha1,
            &target_for_record,
        );
        if let Some(on_disk) = delete_old {
            remove_server_mod_file(&mods, &on_disk)?;
        }
        if redisable {
            // `new_name` is the REAL installed file, so a NotFound here is a true
            // invariant violation — do NOT swallow it.
            let src = mods.join(&new_name);
            let dst = mods.join(format!("{new_name}.disabled"));
            if src.starts_with(&mods) && dst.starts_with(&mods) {
                std::fs::rename(&src, &dst).map_err(|e| Error::io(src.display().to_string(), e))?;
            }
        }
        crate::servers_runtime::installed::remove(&mods, &old_record_sha1)?;
        crate::servers_runtime::installed::upsert(&mods, new_record)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::io("<update-swap>", e))??;
    Ok(report)
}

/// Apply one server-plugin update: install `target` (+ its required dependency
/// closure) into `runtime/plugins/` via the shared plugin kernel, remove the old
/// jar (honoring its `.disabled` suffix), preserve set-aside state, and swap the
/// registry rows. The plugin twin of [`server_update_one`] — differs in the
/// install dir (`plugins`), the kernel (`install_plugin_into_dir`, which pushes
/// the primary FIRST), and the plugin-capable gate. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_update_plugin_one(
    app: AppHandle,
    id: String,
    old_sha1: String,
    target: ModVersion,
) -> Result<crate::mods::dep_resolve::InstallMissingReport> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    if !file.loader.plugin_capable() {
        return Err(Error::ServerCoreUnsupported {
            reason: "this server core does not load plugins".into(),
        });
    }

    // Reconcile the sidecar against disk off the async executor.
    let entries = {
        let dir = p.plugins.clone();
        tokio::task::spawn_blocking(move || {
            crate::servers_runtime::installed::reconcile_on_list(&dir)
        })
        .await
        .map_err(|e| Error::io("<reconcile>", e))??
    };

    let old = entries
        .into_iter()
        .find(|e| e.record.sha1.eq_ignore_ascii_case(&old_sha1))
        .ok_or_else(|| Error::ServerContentStale)?;

    let report = crate::commands::install_plugin_into_dir(
        &base,
        &p.plugins,
        target.source,
        &target.project_id,
        &target.version_id,
        &file.mc_version,
        file.loader,
    )
    .await?;

    // The PLUGIN kernel pushes the primary FIRST (index 0), then appends deps —
    // the mirror of the mods twin's `.last()`. An empty list means the
    // already-installed short-circuit fired: nothing to reconcile.
    let Some(new_name) = report.installed.first().cloned() else {
        return Ok(report);
    };

    // File + registry bookkeeping off the async executor (jar hash + fs + sidecar).
    let plugins = p.plugins.clone();
    let old_enabled = old.enabled;
    let old_base = old.record.filename.clone();
    let old_record_sha1 = old.record.sha1.clone();
    let target_for_record = target.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let new_sha1 = crate::servers_runtime::installed::sha1_of(&plugins.join(&new_name))?;
        let (delete_old, redisable, new_record) = plan_swap(
            old_enabled,
            &old_base,
            &new_name,
            &new_sha1,
            &target_for_record,
        );
        if let Some(on_disk) = delete_old {
            remove_server_mod_file(&plugins, &on_disk)?;
        }
        if redisable {
            let src = plugins.join(&new_name);
            let dst = plugins.join(format!("{new_name}.disabled"));
            if src.starts_with(&plugins) && dst.starts_with(&plugins) {
                std::fs::rename(&src, &dst).map_err(|e| Error::io(src.display().to_string(), e))?;
            }
        }
        crate::servers_runtime::installed::remove(&plugins, &old_record_sha1)?;
        crate::servers_runtime::installed::upsert(&plugins, new_record)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::io("<update-swap>", e))??;
    Ok(report)
}

/// Install a chosen plugin version + its required dependency closure into the
/// server's `runtime/plugins/`. The plugin twin of [`server_install_mod`]:
/// resolves the server's mc_version + core from `server.json`, gates on the core
/// being plugin-capable (Paper/Purpur), then reuses the shared plugin install
/// kernel ([`crate::commands::install_plugin_into_dir`]). Server must be stopped.
/// Returns the jars written + any dependency that could not be resolved.
#[tauri::command]
#[specta::specta]
pub async fn server_install_plugin(
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
    if !file.loader.plugin_capable() {
        return Err(Error::ServerCoreUnsupported {
            reason: "this server core does not load plugins".into(),
        });
    }
    let report = crate::commands::install_plugin_into_dir(
        &base,
        &p.plugins,
        source,
        &project_id,
        &version_id,
        &file.mc_version,
        file.loader,
    )
    .await?;

    // The PLUGIN kernel pushes the primary FIRST (index 0), then appends deps.
    // Best-effort AND off the async executor: `sha1_of` reads the whole jar and
    // `upsert` writes the sidecar — both blocking, so run them in `spawn_blocking`
    // (mirrors `install_local_plugin`). A sidecar failure never fails an install
    // already completed on disk.
    if let Some(primary) = report.installed.first() {
        let dir = p.plugins.clone();
        let primary = primary.clone();
        let (src, pid, vid) = (source, project_id, version_id);
        let _ = tokio::task::spawn_blocking(move || {
            let jar = dir.join(&primary);
            if let Ok(sha1) = crate::servers_runtime::installed::sha1_of(&jar) {
                let _ = crate::servers_runtime::installed::upsert(
                    &dir,
                    crate::servers_runtime::installed::ServerInstalledRecord {
                        filename: primary,
                        sha1: sha1.to_ascii_lowercase(),
                        source: Some(src),
                        project_id: Some(pid),
                        version_id: Some(vid),
                        name: None,
                        version_number: None,
                        enrich_attempted: false,
                    },
                );
            }
        })
        .await;
    }
    Ok(report)
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
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    let stripped = match filename.strip_suffix(".disabled") {
        Some(s) => s.to_string(),
        None => {
            return Err(Error::server_file_invalid(
                filename.as_str(),
                "not a disabled mod",
            ))
        }
    };
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let src = mods.join(&filename);
    let dst = mods.join(&stripped);
    if !src.starts_with(&mods) || !dst.starts_with(&mods) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "path escapes mods dir",
        ));
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

/// Disable (rename to `*.disabled`) a single mod in the server's `mods/`.
/// The mirror of `server_disable_plugin` for the mods dir: a user-initiated
/// disable, so — unlike `server_disable_mods`' client-only quarantine — it
/// writes NO quarantine `reason` sidecar. Single-file, no dependency guard.
/// Rejects unsafe filenames / path escapes. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub fn server_disable_mod(app: AppHandle, id: String, filename: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    let src = mods.join(&filename);
    if !src.starts_with(&mods) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "path escapes mods dir",
        ));
    }
    let dst = mods.join(format!("{filename}.disabled"));
    match std::fs::rename(&src, &dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(src.display().to_string(), e)),
    }
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
        .ok_or_else(|| {
            Error::server_file_invalid(jar_path.as_str(), "dropped path has no filename")
        })?;
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    if !filename.to_ascii_lowercase().ends_with(".jar") {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "mod must be a .jar",
        ));
    }
    let bytes = tokio::fs::read(&jar_path)
        .await
        .map_err(|e| Error::io(jar_path.clone(), e))?;
    // Validate it parses as a jar (a zip) before committing — reject junk.
    if crate::mods::local::read_jar_meta(&bytes).is_err() {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "not a valid mod jar",
        ));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let mods = crate::paths::server_paths(&base, &id).mods;
    tokio::fs::create_dir_all(&mods)
        .await
        .map_err(|e| Error::io(mods.display().to_string(), e))?;
    let dest = mods.join(&filename);
    if !dest.starts_with(&mods) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "path escapes mods dir",
        ));
    }
    tokio::fs::write(&dest, &bytes)
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;
    Ok(filename)
}

/// Raw `server.properties` text for a server (empty when absent) — used to
/// resolve the `level-name` (datapacks live under `runtime/<level>/datapacks/`).
pub(super) fn server_props_raw(p: &crate::paths::ServerPaths) -> String {
    std::fs::read_to_string(p.runtime.join("server.properties")).unwrap_or_default()
}

/// One entry in `server_list_plugins`. Unlike mods there is no quarantine
/// sidecar — plugins have no client/server ambiguity — so no reason field.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerPluginEntry {
    pub filename: String,
    pub disabled: bool,
}

/// `ServerPluginEntry` + the install-identity overlay (no quarantine reason).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServerPluginEntryEnriched {
    pub filename: String,
    /// Current on-disk name (`filename` + `.disabled` when disabled). Mutations
    /// (delete/enable/disable) MUST join this, not the base `filename`.
    pub on_disk_filename: String,
    pub disabled: bool,
    pub sha1: String,
    pub source: Option<crate::mods::platform::ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version_number: Option<String>,
}

/// List the `.jar` / `.jar.disabled` plugins installed for a server's
/// `runtime/plugins/`. Sorted by filename. Missing dir yields an empty list.
#[tauri::command]
#[specta::specta]
pub fn server_list_plugins(app: AppHandle, id: String) -> Result<Vec<ServerPluginEntry>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).plugins;
    Ok(crate::servers_runtime::plugins::list_plugins(&dir)
        .into_iter()
        .map(|(filename, disabled)| ServerPluginEntry { filename, disabled })
        .collect())
}

/// Plugin twin of `server_list_mods_enriched` (no quarantine reason).
#[tauri::command]
#[specta::specta]
pub fn server_list_plugins_enriched(
    app: AppHandle,
    id: String,
) -> Result<Vec<ServerPluginEntryEnriched>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).plugins;
    let entries = crate::servers_runtime::installed::reconcile_on_list(&dir)?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let disabled = !e.enabled;
            // Current on-disk name so mutation commands join the real file, not
            // the base name (a disabled plugin lives at `<name>.jar.disabled`).
            let on_disk = if disabled {
                format!("{}.disabled", e.record.filename)
            } else {
                e.record.filename.clone()
            };
            ServerPluginEntryEnriched {
                filename: e.record.filename,
                on_disk_filename: on_disk,
                disabled,
                sha1: e.record.sha1,
                source: e.record.source,
                project_id: e.record.project_id,
                version_id: e.record.version_id,
                name: e.record.name,
                version_number: e.record.version_number,
            }
        })
        .collect())
}

/// Install a local plugin `.jar` (chosen via the file picker) into the
/// server's `runtime/plugins/`. Mirrors `server_install_local` (path-based —
/// no heavy bytes over IPC). Validates the jar carries `plugin.yml` /
/// `paper-plugin.yml` at its root. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub async fn server_install_plugin_local(
    app: AppHandle,
    id: String,
    jar_path: String,
) -> Result<String> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    // Defense-in-depth: reject a plugin install onto a non-plugin core BEFORE
    // touching the fs, mirroring `require_mod_loader` for the mods path. Reads
    // `server.json` first so a mod-core server never grows a `runtime/plugins/`.
    let file = store::read_server_json(&p.json)?;
    if !file.loader.plugin_capable() {
        return Err(Error::ServerCoreUnsupported {
            reason: "this server core does not load plugins".into(),
        });
    }
    let dir = p.plugins;
    let src = std::path::PathBuf::from(jar_path);
    // `install_local_plugin` does whole-jar blocking std::fs read+write; run it
    // off the async runtime so a large jar can't stall other tasks.
    tokio::task::spawn_blocking(move || {
        crate::servers_runtime::plugins::install_local_plugin(&dir, &src)
    })
    .await
    .map_err(|e| Error::io("<plugin>", format!("join: {e}")))?
}

/// Re-enable a set-aside plugin: rename `<name>.jar.disabled` → `<name>.jar`.
/// Inverse of `server_disable_plugin`. Idempotent (absent → `Ok`). Rejects
/// unsafe filenames / path escapes. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub fn server_enable_plugin(app: AppHandle, id: String, filename: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    let stripped = match filename.strip_suffix(".disabled") {
        Some(s) => s.to_string(),
        None => {
            return Err(Error::server_file_invalid(
                filename.as_str(),
                "not a disabled plugin",
            ))
        }
    };
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).plugins;
    let src = dir.join(&filename);
    let dst = dir.join(&stripped);
    if !src.starts_with(&dir) || !dst.starts_with(&dir) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "path escapes plugins dir",
        ));
    }
    match std::fs::rename(&src, &dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(src.display().to_string(), e)),
    }
}

/// Disable (rename to `*.disabled`) a single plugin in the server's
/// `runtime/plugins/`. Unlike `server_disable_mods` this is single-file: no
/// bulk, no dependency guard (plugins have no dependency graph the launcher
/// tracks). Rejects unsafe filenames / path escapes. Server must be stopped.
#[tauri::command]
#[specta::specta]
pub fn server_disable_plugin(app: AppHandle, id: String, filename: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).plugins;
    let src = dir.join(&filename);
    if !src.starts_with(&dir) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "path escapes plugins dir",
        ));
    }
    let dst = dir.join(format!("{filename}.disabled"));
    match std::fs::rename(&src, &dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(src.display().to_string(), e)),
    }
}

/// Delete a plugin from the server's `runtime/plugins/` by filename.
/// Idempotent: file already gone → `Ok`. Rejects unsafe filenames (path
/// traversal). Refuses while the server is running — symmetric with
/// `server_delete_mod` — so a live plugin's jar is never deleted out from
/// under a running Bukkit-family server.
#[tauri::command]
#[specta::specta]
pub fn server_delete_plugin(app: AppHandle, id: String, filename: String) -> Result<()> {
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "invalid filename",
        ));
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).plugins;
    let path = dir.join(&filename);
    if !path.starts_with(&dir) {
        return Err(Error::server_file_invalid(
            filename.as_str(),
            "path escapes plugins dir",
        ));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

/// Open the server's `runtime/plugins/` folder in the system file manager.
/// Creates the folder if it doesn't exist yet. Mirrors `server_open_logs_folder`.
#[tauri::command]
#[specta::specta]
pub async fn server_open_plugins_folder(app: AppHandle, id: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).plugins;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

/// Open the server's `runtime/mods/` folder in the system file manager.
/// Creates the folder if it doesn't exist yet. Mirrors `server_open_plugins_folder`
/// (and the client's `open_mods_folder`) so the sidebar can drop a mod-loader
/// server's operator directly into its mods directory. `server_open_folder`
/// intentionally lands one level up in `runtime/`.
#[tauri::command]
#[specta::specta]
pub async fn server_open_mods_folder(app: AppHandle, id: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let dir = crate::paths::server_paths(&base, &id).mods;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

/// Switch a server's core. Allowed: Vanilla -> Paper|Purpur,
/// Paper <-> Purpur (checked by `core_switch_allowed` — the UI only offers
/// these). Sequence: guard running -> validate -> fresh backup -> resolve +
/// download the new jar (atomic .part rename; a failed download leaves the
/// old jar AND old server.json untouched) -> re-read + re-validate -> only
/// then swap loader/loader_version. Worlds are never touched: Paper converts
/// them itself on first boot. This command owns the pre-switch snapshot;
/// callers must not create another.
#[tauri::command]
#[specta::specta]
pub async fn server_switch_core(
    app: AppHandle,
    id: String,
    target: crate::servers_runtime::schema::ServerCore,
) -> Result<()> {
    use crate::servers_runtime::schema::{core_switch_allowed, ServerCore};
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    if !core_switch_allowed(file.loader, target) {
        return Err(Error::ServerCoreUnsupported {
            reason: "unsupported core switch".into(),
        });
    }
    // Mandatory fresh backup before anything changes on disk.
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    backup::create_backup(&base, &id, &stamp)?;
    // Resolve the target core's newest build, download over runtime/server.jar
    // (atomic: .part + rename-after-verify, old jar survives any failure).
    let jar = match target {
        ServerCore::Paper => {
            crate::servers_runtime::paper::PaperClient::new()
                .latest_stable_build(&file.mc_version)
                .await?
        }
        ServerCore::Purpur => {
            crate::servers_runtime::purpur::PurpurClient::new()
                .latest_successful_build(&file.mc_version)
                .await?
        }
        // core_switch_allowed only ever admits Paper/Purpur as a target (both
        // arms above cover it); every other variant can't reach this match
        // given the guard above, but we reject explicitly rather than panic —
        // the matrix/catalogue only covers plugin cores.
        ServerCore::Vanilla
        | ServerCore::Fabric
        | ServerCore::Quilt
        | ServerCore::Forge
        | ServerCore::NeoForge => {
            return Err(Error::ServerCoreUnsupported {
                reason: "unsupported core switch".into(),
            });
        }
    };
    crate::network::download::download_no_emit_with(
        &jar.url,
        &p.runtime.join("server.jar"),
        jar.checksum,
        "servers",
    )
    .await?;
    // TOCTOU re-check: the entry guard ran before a potentially long download,
    // and a server started mid-switch must not have its label flipped under
    // the running process. The downloaded jar HAS already overwritten
    // runtime/server.jar at this point, but the old label stays authoritative
    // and `server_redownload_jar` converges the jar back — the same
    // recoverable torn state as a mid-download crash.
    if crate::servers_runtime::runtime::is_running(&id) {
        return Err(Error::ServerAlreadyRunning { id });
    }
    // Re-read server.json rather than reusing `file`: the download above can
    // take a while, and a concurrent rename/heap edit must not be clobbered by
    // writing back the pre-download snapshot. Re-validate the switch against
    // the FRESH loader too — it may have changed while we were downloading.
    let mut fresh = crate::servers_runtime::store::read_server_json(&p.json)?;
    if !core_switch_allowed(fresh.loader, target) {
        return Err(Error::ServerCoreUnsupported {
            reason: "unsupported core switch".into(),
        });
    }
    fresh.loader = target;
    fresh.loader_version = Some(jar.build);
    crate::servers_runtime::store::write_server_json(&p.json, &fresh)?;
    Ok(())
}

/// MC versions the given plugin core publishes builds for. The wizard
/// intersects this SET with the Mojang manifest list (which owns ordering).
#[tauri::command]
#[specta::specta]
pub async fn server_core_versions(
    core: crate::servers_runtime::schema::ServerCore,
) -> Result<Vec<String>> {
    use crate::servers_runtime::schema::ServerCore;
    match core {
        ServerCore::Paper => {
            crate::servers_runtime::paper::PaperClient::new()
                .supported_versions()
                .await
        }
        ServerCore::Purpur => {
            crate::servers_runtime::purpur::PurpurClient::new()
                .supported_versions()
                .await
        }
        // Vanilla/mod cores have no build catalogue of their own here — MC
        // versions for them come from the Mojang manifest directly, not this
        // command. Explicit rejection (no wildcard) per the exhaustive-match
        // convention on ServerCore.
        ServerCore::Vanilla
        | ServerCore::Fabric
        | ServerCore::Quilt
        | ServerCore::Forge
        | ServerCore::NeoForge => Err(Error::ServerCoreUnsupported {
            reason: "core has no version catalogue".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_confirmation_matches_modern_and_legacy_lines() {
        // Modern vanilla / Paper (with typical log prefix).
        assert!(is_save_confirmation(
            "[12:00:01] [Server thread/INFO]: Saved the game"
        ));
        // Pre-1.13 wording.
        assert!(is_save_confirmation(
            "[12:00:01] [Server thread/INFO]: Saved the world"
        ));
        // Case-insensitive.
        assert!(is_save_confirmation("SAVED THE GAME"));
    }

    #[test]
    fn save_confirmation_ignores_unrelated_lines() {
        assert!(!is_save_confirmation(
            "[12:00:00] [Server thread/INFO]: Saving the game (this may take a moment!)"
        ));
        assert!(!is_save_confirmation(
            "[12:00:00] [Server thread/INFO]: Automatic saving is now disabled"
        ));
        assert!(!is_save_confirmation("Steve joined the game"));
    }

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

    #[test]
    fn record_as_installed_maps_version_id_and_name_fallback() {
        let r = crate::servers_runtime::installed::ServerInstalledRecord {
            filename: "m.jar".into(),
            sha1: "ab".into(),
            source: Some(crate::mods::platform::ModSource::Modrinth),
            project_id: Some("p".into()),
            version_id: Some("v".into()),
            name: None,
            version_number: None,
            enrich_attempted: true,
        };
        let im = super::record_as_installed(&r, true);
        assert_eq!(im.version_id.as_deref(), Some("v"));
        assert_eq!(im.name, "m.jar"); // filename fallback when name is None
        assert!(im.enabled);
    }

    fn plan_swap_target() -> crate::mods::platform::ModVersion {
        crate::mods::platform::ModVersion {
            source: crate::mods::platform::ModSource::Modrinth,
            project_id: "proj".into(),
            version_id: "ver".into(),
            name: "Cool Mod".into(),
            version_number: "2.0".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![crate::mods::platform::LoaderKind::Fabric],
            primary_file: crate::mods::platform::ModFile {
                filename: "cool-2.jar".into(),
                url: "https://example/cool-2.jar".into(),
                sha1: Some("CC".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[test]
    fn plan_swap_enabled_same_name() {
        // Re-install wrote the same filename: nothing to delete, stays enabled.
        let (delete_old, redisable, record) =
            super::plan_swap(true, "a.jar", "a.jar", "ab", &plan_swap_target());
        assert_eq!(delete_old, None);
        assert!(!redisable);
        assert_eq!(record.filename, "a.jar");
    }

    #[test]
    fn plan_swap_enabled_diff_name() {
        // Enabled old jar under a different name → delete it, keep new enabled.
        let (delete_old, redisable, _record) =
            super::plan_swap(true, "a.jar", "b.jar", "ab", &plan_swap_target());
        assert_eq!(delete_old.as_deref(), Some("a.jar"));
        assert!(!redisable);
    }

    #[test]
    fn plan_swap_disabled_diff_name() {
        // Set-aside old jar → delete its `.disabled` file, re-disable the new
        // primary, and stamp the new record with the target's identity + the
        // file-derived sha1 (lowercased).
        let (delete_old, redisable, record) =
            super::plan_swap(false, "a.jar", "b.jar", "AB", &plan_swap_target());
        assert_eq!(delete_old.as_deref(), Some("a.jar.disabled"));
        assert!(redisable);
        assert_eq!(record.filename, "b.jar");
        assert_eq!(record.sha1, "ab"); // lowercased
        assert_eq!(
            record.source,
            Some(crate::mods::platform::ModSource::Modrinth)
        );
        assert_eq!(record.project_id.as_deref(), Some("proj"));
        assert_eq!(record.version_id.as_deref(), Some("ver"));
        assert_eq!(record.name.as_deref(), Some("Cool Mod"));
        assert_eq!(record.version_number.as_deref(), Some("2.0"));
        assert!(record.enrich_attempted);
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

#[cfg(test)]
mod enrich_resolve_tests {
    use super::resolve_and_gate;
    use crate::mods::curseforge::FingerprintFile;
    use crate::mods::modrinth::HashVersion;
    use crate::mods::platform::ModSource;
    use std::collections::{HashMap, HashSet};

    fn mr(project: &str) -> HashVersion {
        HashVersion {
            project_id: project.into(),
            version_id: "mr-ver".into(),
            version_number: "1.0.0".into(),
            name: "Modrinth Mod".into(),
        }
    }

    fn cf(project: &str) -> FingerprintFile {
        FingerprintFile {
            project_id: project.into(),
            version_id: "cf-file".into(),
            version_number: Some("cf-1.2".into()),
        }
    }

    #[test]
    fn modrinth_wins_tie_break() {
        let shas = vec!["aa".to_string()];
        let mr_hits = HashMap::from([("aa".to_string(), mr("mr-proj"))]);
        let cf_hits = HashMap::from([("aa".to_string(), cf("cf-proj"))]);
        let (resolved, _) = resolve_and_gate(&shas, &mr_hits, true, &cf_hits, true, true);
        let id = resolved.get("aa").expect("sha resolved");
        assert_eq!(id.source, ModSource::Modrinth);
        assert_eq!(id.project_id, "mr-proj");
    }

    #[test]
    fn cf_only_hit_resolves_curseforge() {
        let shas = vec!["bb".to_string()];
        let mr_hits: HashMap<String, HashVersion> = HashMap::new();
        let cf_hits = HashMap::from([("bb".to_string(), cf("cf-proj"))]);
        let (resolved, _) = resolve_and_gate(&shas, &mr_hits, true, &cf_hits, true, true);
        let id = resolved.get("bb").expect("sha resolved");
        assert_eq!(id.source, ModSource::Curseforge);
        assert_eq!(id.project_id, "cf-proj");
        assert!(id.name.is_none());
        assert_eq!(id.version_number.as_deref(), Some("cf-1.2"));
    }

    #[test]
    fn gate_marks_attempted_when_all_tried_ok() {
        // Modrinth-only path (plugin dir / no CF key): cf_tried=false is a
        // vacuous success, so the whole scope is marked attempted.
        let shas = vec!["aa".to_string(), "bb".to_string()];
        let mr_hits: HashMap<String, HashVersion> = HashMap::new();
        let cf_hits: HashMap<String, FingerprintFile> = HashMap::new();
        let (_, attempted) = resolve_and_gate(&shas, &mr_hits, true, &cf_hits, true, false);
        let expected: HashSet<String> = shas.into_iter().collect();
        assert_eq!(attempted, expected);
    }

    #[test]
    fn gate_skips_attempted_on_platform_failure() {
        // Modrinth failed (mr_ok=false) → nothing is marked attempted, but any
        // identity that DID resolve (e.g. from CF) still lands.
        let shas = vec!["aa".to_string()];
        let mr_hits: HashMap<String, HashVersion> = HashMap::new();
        let cf_hits = HashMap::from([("aa".to_string(), cf("cf-proj"))]);
        let (resolved, attempted) = resolve_and_gate(&shas, &mr_hits, false, &cf_hits, true, true);
        assert!(attempted.is_empty());
        assert!(
            resolved.contains_key("aa"),
            "partial-success identity lands"
        );
    }

    #[test]
    fn unmatched_sha_is_neither_resolved_nor_forced() {
        let shas = vec!["cc".to_string()];
        let mr_hits: HashMap<String, HashVersion> = HashMap::new();
        let cf_hits: HashMap<String, FingerprintFile> = HashMap::new();
        let (resolved, _) = resolve_and_gate(&shas, &mr_hits, true, &cf_hits, true, true);
        assert!(!resolved.contains_key("cc"));
    }

    #[test]
    fn gate_skips_attempted_when_cf_failed_though_mr_ok() {
        // CF was tried and failed (cf_ok=false) even though Modrinth succeeded:
        // the whole scope must stay un-attempted so the next pass retries CF.
        let shas = vec!["aa".to_string()];
        let mr_hits: HashMap<String, HashVersion> = HashMap::new();
        let cf_hits: HashMap<String, FingerprintFile> = HashMap::new();
        let (_resolved, attempted) = resolve_and_gate(&shas, &mr_hits, true, &cf_hits, false, true);
        assert!(
            attempted.is_empty(),
            "cf failure must leave the scope un-attempted for retry"
        );
    }
}
