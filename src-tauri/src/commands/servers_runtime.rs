//! Tauri-команды фичи «Свой сервер» (План 1: создание/список/удаление).

use crate::error::Result;
use crate::instances::schema::LoaderKind;
use crate::servers_runtime::schema::{ServerFile, ServerWithStatus};
use crate::servers_runtime::{create, store};
use tauri::AppHandle;

/// Создать vanilla-сервер: разрешить jar через манифест Mojang, скачать,
/// записать `server.json` + `eula.txt`. Другие лоадеры добавляются в Задаче 12.
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
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
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
    // Plan 1: vanilla only. Other loaders will be wired in Task 12.
    let (jar_url, sha1) = create::resolve_vanilla_jar(&file.mc_version).await?;
    create::create_vanilla_server(&base, &file, &jar_url, &sha1).await?;
    Ok(ServerWithStatus::from_file(&file, false, None, None))
}

/// Перечислить все серверы в `<app_data>/servers/`. Живой статус (running/pid/port)
/// будет добавлен в Задаче 14 (процессный менеджер); сейчас всегда `false/None/None`.
#[tauri::command]
#[specta::specta]
pub fn server_list(app: AppHandle) -> Result<Vec<ServerWithStatus>> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    let files = store::list_all(&base)?;
    Ok(files
        .iter()
        .map(|f| ServerWithStatus::from_file(f, false, None, None))
        .collect())
}

/// Удалить сервер и все его данные. Идемпотентно (уже удалён → Ok).
#[tauri::command]
#[specta::specta]
pub fn server_delete(app: AppHandle, id: String) -> Result<()> {
    let base = crate::paths::app_dir(&app).map_err(|e| crate::error::Error::io("<app_dir>", e))?;
    store::delete_server(&base, &id)
}
