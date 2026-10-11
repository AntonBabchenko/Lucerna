//! Choosing the Java an instance, a server or a Java major runs on (spec
//! 2026-10-11-custom-java §6.1, §6.2, §7).

use crate::error::Error;
use crate::instances::schema::{GeneralSettings, InstanceWithStatus};
use crate::java::detect::DetectedJava;
use crate::java::{JavaChoice, JavaDefault, JavaOrigin, JavaReport, JavaUnusableReason, Owner};
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

pub(crate) fn unusable(path: &str, origin: JavaOrigin, reason: JavaUnusableReason) -> Error {
    Error::JavaUnusable {
        path: path.to_string(),
        origin,
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

/// Probe a path against an optional requirement, without saving it. The
/// origin on a refusal is nominal: the UI shows the reason, not the origin.
#[tauri::command]
#[specta::specta]
pub async fn java_check(path: String, required_major: Option<u32>) -> Result<JavaReport, Error> {
    let origin = JavaOrigin::Instance;
    let exe = crate::java::path::normalize(&path).map_err(|r| unusable(&path, origin, r))?;
    crate::java::check(&exe, required_major)
        .await
        .map_err(|r| unusable(&exe.to_string_lossy(), origin, r))
}

/// The version JSON on disk for a version id: `Ok(None)` when it is not
/// installed, an error when it is there but unreadable.
async fn version_details_on_disk(
    app: &tauri::AppHandle,
    version_id: &str,
) -> Result<Option<crate::versions::version_json::VersionDetails>, Error> {
    let path = crate::versions::install::version_json_path(app, version_id)?;
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(t) => t,
        // Not installed yet: the need is unknown, and launch checks it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    crate::versions::version_json::parse(&text)
        .map(Some)
        .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))
}

async fn requirement(app: &tauri::AppHandle, id: &str) -> Result<Option<u32>, Error> {
    let instance = crate::instances::read_instance(app, id)?;
    let Some(effective) = crate::instances::status::effective_version_id(&instance) else {
        return Ok(None);
    };
    Ok(version_details_on_disk(app, &effective)
        .await?
        .map(|d| crate::java::required_major(&d)))
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
    let origin = JavaOrigin::Instance;
    let (stored, report) = match choice {
        Some(JavaChoice::Custom { path }) => {
            let exe =
                crate::java::path::normalize(&path).map_err(|r| unusable(&path, origin, r))?;
            let required = requirement(&app, &id).await?;
            let report = crate::java::check(&exe, required)
                .await
                .map_err(|r| unusable(&exe.to_string_lossy(), origin, r))?;
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

/// The Java majors the installed instances need, for Settings' rows. Reads
/// the version JSONs on disk only. An instance that is not installed, or whose
/// JSON cannot be read, contributes nothing (logged): this lists rows to offer,
/// and a saved row is shown whatever this returns.
#[tauri::command]
#[specta::specta]
pub async fn java_required_majors(app: tauri::AppHandle) -> Result<Vec<u32>, Error> {
    let instances_dir =
        crate::paths::instances_dir(&app).map_err(|e| Error::io("<instances_dir>", e))?;
    let versions_dir =
        crate::paths::versions_dir(&app).map_err(|e| Error::io("<versions_dir>", e))?;
    tokio::task::spawn_blocking(move || {
        let mut majors = std::collections::BTreeSet::new();
        for instance in crate::instances::scan::list_all(&instances_dir) {
            let Some(effective) = crate::instances::status::effective_version_id(&instance) else {
                continue;
            };
            let path = crate::versions::install::version_json_in(&versions_dir, &effective);
            match std::fs::read_to_string(&path) {
                Ok(text) => match crate::versions::version_json::parse(&text) {
                    Ok(d) => {
                        majors.insert(crate::java::required_major(&d));
                    }
                    Err(e) => crate::diag!("java majors: {} unparsable: {e}", path.display()),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => crate::diag!("java majors: {} unreadable: {e}", path.display()),
            }
        }
        majors.into_iter().collect()
    })
    .await
    .map_err(|e| Error::io("<java_required_majors>", e))
}

/// Replace (or with `None`, remove) one row of Settings' per-major Java. A
/// path is normalised and probed against `major` FIRST, outside the
/// `app.json` lock (its closure is synchronous); an unusable one is refused and
/// nothing is written. Returns the settings as they are after the write.
#[tauri::command]
#[specta::specta]
pub async fn set_java_default(
    app: tauri::AppHandle,
    major: u32,
    path: Option<String>,
) -> Result<JavaDefaultSaved, Error> {
    let origin = JavaOrigin::Default { major };
    let (row, report) = match path {
        Some(raw) => {
            let exe = crate::java::path::normalize(&raw).map_err(|r| unusable(&raw, origin, r))?;
            let report = crate::java::check(&exe, Some(major))
                .await
                .map_err(|r| unusable(&exe.to_string_lossy(), origin, r))?;
            (
                Some(JavaDefault {
                    major,
                    path: exe.to_string_lossy().into_owned(),
                }),
                Some(report),
            )
        }
        None => (None, None),
    };
    let file = crate::paths::app_file(&app).map_err(|e| Error::io("<app_file>", e))?;
    let af = crate::instances::store::update_app_json(&file, |af| {
        let next = with_row(&af.general.java_defaults, major, row);
        if next == af.general.java_defaults {
            return crate::instances::store::Verdict::Unchanged;
        }
        af.general.java_defaults = next;
        crate::instances::store::Verdict::Write
    })?;
    Ok(JavaDefaultSaved {
        settings: af.general,
        report,
    })
}

/// What `set_java_default` saved: the settings as persisted, and the probe's
/// report for the row's Java (`None` when the row was removed).
#[derive(Debug, Clone, Serialize, Type)]
pub struct JavaDefaultSaved {
    pub settings: GeneralSettings,
    pub report: Option<JavaReport>,
}

/// The rows with `major`'s replaced by `row` (or removed), unique by major and
/// sorted. Pure.
fn with_row(rows: &[JavaDefault], major: u32, row: Option<JavaDefault>) -> Vec<JavaDefault> {
    let mut next: Vec<JavaDefault> = rows.iter().filter(|r| r.major != major).cloned().collect();
    next.extend(row);
    next.sort_by_key(|r| r.major);
    next
}

/// What phase 1b of an instance's install should do with Lucerna's runtime:
/// skip it for an instance that resolves to a usable Java of its own (or of
/// Settings), install it otherwise. Anything that cannot be told — unreadable
/// Settings, a chosen Java that fails its probe — installs it: downloading a
/// runtime runs nothing, and the launch still refuses a chosen Java that
/// fails, so the restrictive answer here is "install".
pub(crate) async fn install_jre_phase(
    app: &tauri::AppHandle,
    instance_id: &str,
    effective_id: &str,
) -> Result<crate::versions::install::JrePhase, Error> {
    use crate::versions::install::JrePhase;
    let instance = crate::instances::read_instance(app, instance_id)?;
    let details = crate::versions::install::ensure_version_json(effective_id, app).await?;
    let required = crate::java::required_major(&details);
    let defaults = match crate::java::defaults_for(instance.java.as_ref(), || {
        crate::java::settings_defaults(app)
    }) {
        Ok(d) => d,
        Err(e) => {
            crate::diag!("install: {e}; installing Lucerna's Java");
            return Ok(JrePhase::Install);
        }
    };
    match crate::java::resolve(instance.java.as_ref(), Owner::Instance, &defaults, required) {
        crate::java::JavaSource::Managed => Ok(JrePhase::Install),
        crate::java::JavaSource::Custom { path, .. } => {
            let Ok(exe) = crate::java::path::normalize(&path.to_string_lossy()) else {
                return Ok(JrePhase::Install);
            };
            match crate::java::check(&exe, Some(required)).await {
                Ok(_) => Ok(JrePhase::Skip { java: exe }),
                Err(reason) => {
                    crate::diag!(
                        "install: the chosen Java {} cannot run this ({reason:?}); installing Lucerna's Java",
                        exe.display()
                    );
                    Ok(JrePhase::Install)
                }
            }
        }
    }
}

/// Download Lucerna's runtime for an instance's version — the action behind
/// `Error::ManagedJavaMissing`. Only the runtime: not a full install, which
/// would re-hash every library and asset.
#[tauri::command]
#[specta::specta]
pub async fn ensure_instance_jre(app: tauri::AppHandle, id: String) -> Result<(), Error> {
    crate::data_root::reject_if_root_unusable(&app)?;
    let instance = crate::instances::read_instance(&app, &id)?;
    let effective = crate::instances::status::effective_version_id(&instance)
        .ok_or(Error::NoVersionSelected)?;
    let details = version_details_on_disk(&app, &effective)
        .await?
        .ok_or(Error::NoVersionSelected)?;
    let component = details
        .java_version
        .as_ref()
        .map(|jv| jv.component.clone())
        .unwrap_or_else(|| crate::jre::DEFAULT_LEGACY_COMPONENT.to_string());
    let major = crate::java::required_major(&details);
    crate::jre::ensure_jre(&component, &app, |_, _, _| {})
        .await
        .map_err(|e| {
            crate::jre::unavailable(
                e,
                &component,
                major,
                crate::error::JreStage::GameRuntime,
                &app,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(major: u32, path: &str) -> JavaDefault {
        JavaDefault {
            major,
            path: path.into(),
        }
    }

    #[test]
    fn a_row_replaces_its_major_and_keeps_the_others_sorted() {
        let rows = vec![row(21, "/a"), row(8, "/b")];
        assert_eq!(
            with_row(&rows, 21, Some(row(21, "/c"))),
            vec![row(8, "/b"), row(21, "/c")]
        );
    }

    #[test]
    fn none_removes_the_row() {
        assert_eq!(with_row(&[row(17, "/a")], 17, None), vec![]);
    }

    #[test]
    fn a_new_major_is_added() {
        assert_eq!(
            with_row(&[row(17, "/a")], 8, Some(row(8, "/b"))),
            vec![row(8, "/b"), row(17, "/a")]
        );
    }
}
