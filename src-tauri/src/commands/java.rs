//! Choosing the Java an instance runs on (spec 2026-10-11-custom-java §6.1).

use crate::error::Error;
use crate::instances::schema::InstanceWithStatus;
use crate::java::detect::DetectedJava;
use crate::java::{JavaChoice, JavaReport, JavaUnusableReason};
use serde::Serialize;
use specta::Type;
use std::sync::{Mutex, MutexGuard};

/// Detection results for this session. A picker opening twice does not walk
/// the disk twice; its Refresh button does.
static DETECTED: Mutex<Option<Vec<DetectedJava>>> = Mutex::new(None);

fn detected_cache() -> MutexGuard<'static, Option<Vec<DetectedJava>>> {
    // A panic while holding this lock leaves a list that is at worst stale;
    // recover it rather than fail every later picker.
    DETECTED.lock().unwrap_or_else(|p| p.into_inner())
}

fn unusable(path: &str, reason: JavaUnusableReason) -> Error {
    Error::JavaUnusable {
        path: path.to_string(),
        reason,
    }
}

/// The Javas installed on this machine, Lucerna's own runtimes excluded.
#[tauri::command]
#[specta::specta]
pub async fn java_detect(app: tauri::AppHandle, refresh: bool) -> Result<Vec<DetectedJava>, Error> {
    if !refresh {
        if let Some(cached) = detected_cache().clone() {
            return Ok(cached);
        }
    }
    let jres = crate::paths::jres_dir(&app).map_err(|e| Error::io("<jres_dir>", e))?;
    let found = tokio::task::spawn_blocking(move || {
        // `jres/` may not exist yet (nothing installed). Then exclude the raw
        // path: nothing can be found under a directory that is not there.
        let exclude = dunce::canonicalize(&jres).unwrap_or(jres);
        crate::java::detect::collect(&crate::java::detect::default_roots(), &exclude)
    })
    .await
    .map_err(|e| Error::io("<java_detect>", e))?;
    *detected_cache() = Some(found.clone());
    Ok(found)
}

/// Probe a path against an optional requirement, without saving it.
#[tauri::command]
#[specta::specta]
pub async fn java_check(path: String, required_major: Option<u32>) -> Result<JavaReport, Error> {
    let exe = crate::java::path::normalize(&path).map_err(|r| unusable(&path, r))?;
    crate::java::check(&exe, required_major)
        .await
        .map_err(|r| unusable(&exe.to_string_lossy(), r))
}

async fn requirement(app: &tauri::AppHandle, id: &str) -> Result<Option<u32>, Error> {
    let instance = crate::instances::read_instance(app, id)?;
    let Some(effective) = crate::instances::status::effective_version_id(&instance) else {
        return Ok(None);
    };
    let path = crate::versions::install::version_json_path(app, &effective)?;
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(t) => t,
        // Not installed yet: the need is unknown, and launch checks it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    let details = crate::versions::version_json::parse(&text)
        .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))?;
    Ok(Some(crate::java::required_major(&details)))
}

/// The Java major an instance's installed version needs; `None` when it is not
/// installed yet. Reads the version JSON on disk — the one launch reads — and
/// never the network.
#[tauri::command]
#[specta::specta]
pub async fn instance_java_requirement(
    app: tauri::AppHandle,
    id: String,
) -> Result<Option<u32>, Error> {
    requirement(&app, &id).await
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct JavaSaved {
    pub instance: InstanceWithStatus,
    /// The probe's report for a custom Java; `None` for Lucerna's runtime.
    pub report: Option<JavaReport>,
}

/// Store an instance's Java. A custom path is normalised and probed against
/// the instance's need FIRST; an unusable one is refused and nothing is
/// written. Not behind the maintenance gate, like the heap and the JVM
/// arguments: it changes no file the game or a migration touches, and it
/// applies from the next launch.
#[tauri::command]
#[specta::specta]
pub async fn set_instance_java(
    app: tauri::AppHandle,
    id: String,
    choice: Option<JavaChoice>,
) -> Result<JavaSaved, Error> {
    let (stored, report) = match choice {
        Some(JavaChoice::Custom { path }) => {
            let exe = crate::java::path::normalize(&path).map_err(|r| unusable(&path, r))?;
            let required = requirement(&app, &id).await?;
            let report = crate::java::check(&exe, required)
                .await
                .map_err(|r| unusable(&exe.to_string_lossy(), r))?;
            (
                Some(JavaChoice::Custom {
                    path: exe.to_string_lossy().into_owned(),
                }),
                Some(report),
            )
        }
        other => (other, None),
    };
    let instance = crate::instances::set_instance_java(&app, &id, stored)?;
    Ok(JavaSaved { instance, report })
}
