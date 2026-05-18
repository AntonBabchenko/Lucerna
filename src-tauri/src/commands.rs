use specta::Type;

#[derive(Debug, serde::Serialize, Type)]
pub struct Greeting {
    pub message: String,
}

const MAX_INSTANCE_NAME_LEN: u32 = 32;

/// Validate instance name at the IPC boundary.
///
/// Reasons live as typed Error variants so the UI doesn't string-parse.
/// Count uses unicode scalar values (chars), not bytes — a 32-char
/// cyrillic name is 64 bytes but 32 graphemes, and that's fine.
fn validate_instance_name(name: &str) -> Result<(), crate::error::Error> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::error::Error::InstanceNameEmpty);
    }
    let count = trimmed.chars().count() as u32;
    if count > MAX_INSTANCE_NAME_LEN {
        return Err(crate::error::Error::InstanceNameTooLong {
            max: MAX_INSTANCE_NAME_LEN,
            actual: count,
        });
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — FTlauncher is alive."),
    }
}

/// Return the most recent network activity entries (up to 200).
#[tauri::command]
#[specta::specta]
pub fn network_activity() -> Vec<crate::network::AuditEntry> {
    crate::network::recent()
}

/// Audit entries whose host is NOT on the documented allowlist
/// (per `docs/PRINCIPLES.md` Part A item #2). UI shows a red banner
/// in the Network popover when this returns non-empty.
#[tauri::command]
#[specta::specta]
pub fn network_audit_violations() -> Vec<crate::network::AuditEntry> {
    crate::network::audit_violations()
}

/// List all stored accounts.
#[tauri::command]
#[specta::specta]
pub fn list_accounts(app: tauri::AppHandle) -> Result<Vec<crate::accounts::store::Account>, crate::error::Error> {
    crate::accounts::list_accounts(&app)
}

/// Currently active account, or None if no account is set.
#[tauri::command]
#[specta::specta]
pub fn get_active_account(
    app: tauri::AppHandle,
) -> Result<Option<crate::accounts::store::Account>, crate::error::Error> {
    crate::accounts::get_active_account(&app)
}

/// Set the active account by id. Errors AccountNotSet if id is unknown.
#[tauri::command]
#[specta::specta]
pub fn set_active_account(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::accounts::set_active_account(&app, &id)
}

/// Remove an account. If it was active, the next account becomes active;
/// if none remain, active_id becomes None.
#[tauri::command]
#[specta::specta]
pub fn remove_account(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::accounts::remove_account(&app, &id)
}

/// Add an offline account. Idempotent — same name produces same UUID.
#[tauri::command]
#[specta::specta]
pub fn add_offline_account(
    app: tauri::AppHandle,
    name: String,
) -> Result<crate::accounts::store::Account, crate::error::Error> {
    crate::accounts::add_offline_account(&app, &name)
}

/// Fetch the Mojang version manifest. Cached for 5 minutes — repeated
/// calls within that window are zero-network.
#[tauri::command]
#[specta::specta]
pub async fn list_versions() -> Result<Vec<crate::versions::VersionEntry>, crate::error::Error> {
    crate::versions::list_manifest().await
}

/// Install a Minecraft version: downloads the per-version JSON,
/// libraries, assets, and client.jar. Emits `installProgress` events
/// throughout. Idempotent — files already present with matching SHA-1
/// are skipped.
#[tauri::command]
#[specta::specta]
pub async fn install_version(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<(), crate::error::Error> {
    crate::versions::install_version(&version_id, &app).await
}

/// Install (idempotently) the given instance's version. Does NOT launch
/// — the UI shows an Install button when the instance is not ready and
/// a Play button once it is. Emits `installProgress` during the run.
/// Resolves version+loader from `instance.json` server-side.
#[tauri::command]
#[specta::specta]
pub async fn install_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    crate::versions::install_version(&effective_id, &app).await
}

/// Launch the given instance. Assumes it is already installed (the UI
/// only enables Play when `instance.ready == true`, which checks the
/// effective version's profile JSON + parent MC client jar). Does NOT
/// re-run install_version — earlier attempts to add it as a safety net
/// re-hashed every library and asset on every click, surfacing as a
/// visible "Downloading…" flash. If launch fails on missing files, the
/// error surfaces and the user can click Install (Manage → re-pick the
/// loader, or the Install button reappears once ready_status flips).
#[tauri::command]
#[specta::specta]
pub async fn launch_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<u32, crate::error::Error> {
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let json_path = crate::paths::instance_json(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    let account = crate::accounts::get_active_account(&app)?
        .ok_or(crate::error::Error::AccountNotSet)?;
    crate::launch::start(&instance, &effective_id, &account, &app).await
}

/// Shared prelude for install_instance and launch_instance: confirm the
/// instance exists, read its JSON, and resolve the effective version id.
/// Returns the version id only; callers that need the full Instance read
/// it again (cheap; same file on disk).
fn resolve_instance_effective_id(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<String, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == instance_id) {
        return Err(crate::error::Error::InstanceNotFound {
            id: instance_id.to_string(),
        });
    }
    let json_path = crate::paths::instance_json(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    crate::instances::status::effective_version_id(&instance)
        .ok_or(crate::error::Error::NoVersionSelected)
}

/// Kill the running Minecraft process if any. Idempotent.
#[tauri::command]
#[specta::specta]
pub fn stop_minecraft() -> Result<(), crate::error::Error> {
    crate::launch::stop()
}

/// List every log file under `instance_id`'s three documented roots.
/// Sorted by mtime descending.
#[tauri::command]
#[specta::specta]
pub fn list_log_files(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::logs::files::LogFileMeta>, crate::error::Error> {
    crate::logs::files::list_log_files(&app, &instance_id)
}

/// Read up to `max_bytes` of a log file. `max_bytes` is clamped to
/// `[64 KB, 100 MB]`; `0` becomes the 5 MB default. `path` must be
/// under one of SOME instance's allowed log roots — anything else is
/// rejected with `Error::Io`.
#[tauri::command]
#[specta::specta]
pub fn read_log_file(
    app: tauri::AppHandle,
    path: String,
    max_bytes: f64,
) -> Result<String, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(&app)?;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for inst in &all {
        let mut r = crate::logs::files::allowed_roots(&app, &inst.id)?;
        roots.append(&mut r);
    }
    let path = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let cap = if !max_bytes.is_finite() || max_bytes < 0.0 {
        0
    } else {
        max_bytes as u64
    };
    crate::logs::read::read_with_cap(&path, cap)
}

/// Newest crash report (if any) for `instance_id`. Used by the UI to
/// show a banner on non-zero MC exit.
#[tauri::command]
#[specta::specta]
pub fn latest_crash(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Option<crate::logs::files::CrashReport>, crate::error::Error> {
    crate::logs::files::latest_crash(&app, &instance_id)
}

/// Ensure `<instance>/.minecraft/mods/` exists, then open it in the OS
/// file manager. Idempotent — safe to click repeatedly. Vanilla MC
/// does not load mods; the UI carries a caveat below the button.
#[tauri::command]
#[specta::specta]
pub async fn open_mods_folder(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    let dir = crate::paths::mods_dir(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<mods_dir>", e))?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| {
            crate::error::Error::io(dir.display().to_string(), format!("opener: {e}"))
        })?;
    Ok(())
}

/// List Fabric loader versions compatible with `mc_id`. Sorted
/// newest-first by build. Empty list → `Error::LoaderUnavailable`.
/// Cached 5 minutes per `mc_id`.
#[tauri::command]
#[specta::specta]
pub async fn list_fabric_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::Fabric, &mc_id).await
}

/// List Quilt loader versions compatible with `mc_id`. Same semantics
/// as `list_fabric_loaders`. Stability is inferred from the version
/// string (Quilt meta does not expose a `stable` flag).
#[tauri::command]
#[specta::specta]
pub async fn list_quilt_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::Quilt, &mc_id).await
}

/// List Forge loader versions compatible with `mc_id`. Cached
/// 5 minutes per MC version. Empty list → `LoaderUnavailable`.
#[tauri::command]
#[specta::specta]
pub async fn list_forge_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::Forge, &mc_id).await
}

/// List NeoForge loader versions compatible with `mc_id`. Cached
/// 5 minutes per MC version. Empty list → `LoaderUnavailable`.
#[tauri::command]
#[specta::specta]
pub async fn list_neoforge_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::NeoForge, &mc_id).await
}

/// All instances on disk with precomputed `ready` status. Sorted
/// oldest-first by `created_unix_ms`.
#[tauri::command]
#[specta::specta]
pub fn list_instances(
    app: tauri::AppHandle,
) -> Result<Vec<crate::instances::schema::InstanceWithStatus>, crate::error::Error> {
    crate::instances::list_instances_with_status(&app)
}

/// Currently active instance, or `None` if no instances exist.
#[tauri::command]
#[specta::specta]
pub fn get_active_instance(
    app: tauri::AppHandle,
) -> Result<Option<crate::instances::schema::InstanceWithStatus>, crate::error::Error> {
    crate::instances::get_active_instance(&app)
}

/// Set the active instance by id. Errors `InstanceNotFound` if id is unknown.
#[tauri::command]
#[specta::specta]
pub fn set_active_instance(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::error::Error> {
    crate::instances::set_active_instance(&app, &id)
}

/// Create a new instance. Generates a UUID, mkdirs `.minecraft/`,
/// writes `instance.json`. Defaults: `max_heap_mb=2048`, `extra_jvm_args=""`.
#[tauri::command]
#[specta::specta]
pub fn create_instance(
    app: tauri::AppHandle,
    name: String,
    mc_version: String,
    loader: crate::instances::schema::LoaderKind,
    loader_version: Option<String>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    validate_instance_name(&name)?;
    crate::instances::create_instance(&app, name, mc_version, loader, loader_version)
}

/// Delete an instance. If it was active, auto-switches to oldest remaining.
/// Errors `LastInstance` if it's the only one left.
#[tauri::command]
#[specta::specta]
pub fn delete_instance(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::error::Error> {
    crate::instances::delete_instance(&app, &id)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_name(
    app: tauri::AppHandle,
    id: String,
    name: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    validate_instance_name(&name)?;
    crate::instances::set_instance_name(&app, &id, name)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_version(
    app: tauri::AppHandle,
    id: String,
    mc_version: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_version(&app, &id, mc_version)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_loader(
    app: tauri::AppHandle,
    id: String,
    loader: crate::instances::schema::LoaderKind,
    loader_version: Option<String>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_loader(&app, &id, loader, loader_version)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_memory(
    app: tauri::AppHandle,
    id: String,
    max_heap_mb: u32,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_memory(&app, &id, max_heap_mb)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_jvm_args(
    app: tauri::AppHandle,
    id: String,
    args: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_jvm_args(&app, &id, args)
}

/// Ensure `<instance>/.minecraft/` exists, then open it in the OS
/// file manager.
#[tauri::command]
#[specta::specta]
pub async fn open_instance_folder(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::error::Error> {
    crate::instances::open_instance_folder(&app, &id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn greet_includes_name() {
        let g = greet("World".to_string());
        assert!(g.message.contains("World"));
        assert!(g.message.contains("FTlauncher"));
    }

    #[test]
    fn validate_accepts_normal_name() {
        assert!(validate_instance_name("My Pack").is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(matches!(
            validate_instance_name(""),
            Err(Error::InstanceNameEmpty)
        ));
    }

    #[test]
    fn validate_rejects_whitespace_only() {
        assert!(matches!(
            validate_instance_name("   \t  "),
            Err(Error::InstanceNameEmpty)
        ));
    }

    #[test]
    fn validate_trims_leading_trailing_whitespace_for_length_check() {
        // 32 chars surrounded by spaces — trimmed length is 32, valid.
        let name = format!("  {}  ", "a".repeat(32));
        assert!(validate_instance_name(&name).is_ok());
    }

    #[test]
    fn validate_accepts_exactly_32_chars() {
        assert!(validate_instance_name(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn validate_rejects_33_chars() {
        let result = validate_instance_name(&"a".repeat(33));
        assert!(matches!(
            result,
            Err(Error::InstanceNameTooLong { max: 32, actual: 33 })
        ));
    }

    #[test]
    fn validate_counts_unicode_scalar_values_not_bytes() {
        // 30 cyrillic chars = 60 bytes in UTF-8 but 30 scalars — valid.
        assert!(validate_instance_name(&"я".repeat(30)).is_ok());
        // 33 cyrillic chars = 66 bytes — should still reject as 33 too long.
        let result = validate_instance_name(&"я".repeat(33));
        assert!(matches!(
            result,
            Err(Error::InstanceNameTooLong { max: 32, actual: 33 })
        ));
    }

    // These tests verify the validate_instance_name call site — they do
    // NOT exercise the full Tauri command path (no AppHandle available
    // in unit tests). For full integration use the matrix harness.

    #[test]
    fn validate_rejects_at_create_call_site_path() {
        // The shape we want: anyone calling validate_instance_name
        // before reaching crate::instances::create_instance gets the
        // correct typed error. The function's a private guard, so this
        // is a behavioural assertion via the public helper.
        let r = validate_instance_name("");
        assert!(matches!(r, Err(Error::InstanceNameEmpty)));
        let r = validate_instance_name(&"x".repeat(33));
        assert!(matches!(r, Err(Error::InstanceNameTooLong { .. })));
    }
}
