//! Classifies whether an imported modpack instance has a newer published
//! version. The two pure pieces here are unit-tested without a Tauri
//! AppHandle; the command layer (`commands::modpack_cmds`) wires them to
//! `read_instance` + `modpack_source_for(..).get_versions(..)`.

use serde::Serialize;
use specta::Type;

use crate::mods::modpack::schema::ModpackVersionEntry;
use crate::mods::platform::ModSource;

/// Why a pack instance cannot be update-checked (structural, not transient).
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotCheckableReason {
    /// Manually-created instance — never had a modpack source.
    NotAPack,
    /// Drag-drop import: no recorded project_id / version_id to compare against.
    NoProvenance,
    /// CurseForge pack but no API key is stored, so versions can't be listed.
    NeedsCurseforgeKey,
}

/// The resolved update state for one pack instance.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModpackUpdateStatus {
    UpToDate,
    UpdateAvailable { entry: ModpackVersionEntry },
    NotCheckable { reason: NotCheckableReason },
    CheckFailed { message: String },
}

/// One batch entry: an instance id paired with its resolved status.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackInstanceUpdate {
    pub instance_id: String,
    pub status: ModpackUpdateStatus,
}

/// Pure precondition check. Returns `Ok((source, project_id, version_id))`
/// when a network check should proceed, or `Err(status)` carrying the
/// terminal `NotCheckable` status when it should not.
// Err is boxed to keep precheck's Result small (clippy::result_large_err); the
// unboxed status is recovered at the call site.
pub fn precheck(
    source: Option<ModSource>,
    project_id: Option<&str>,
    version_id: Option<&str>,
    cf_key_present: bool,
) -> Result<(ModSource, String, String), Box<ModpackUpdateStatus>> {
    let Some(source) = source else {
        return Err(Box::new(ModpackUpdateStatus::NotCheckable {
            reason: NotCheckableReason::NotAPack,
        }));
    };
    let (Some(pid), Some(vid)) = (project_id, version_id) else {
        return Err(Box::new(ModpackUpdateStatus::NotCheckable {
            reason: NotCheckableReason::NoProvenance,
        }));
    };
    if source == ModSource::Curseforge && !cf_key_present {
        return Err(Box::new(ModpackUpdateStatus::NotCheckable {
            reason: NotCheckableReason::NeedsCurseforgeKey,
        }));
    }
    Ok((source, pid.to_string(), vid.to_string()))
}

/// Pure: given a fetched version list and the instance's current version id,
/// decide `UpToDate` vs `UpdateAvailable`. Wraps `commands::latest_newer`.
pub fn status_from_versions(
    versions: Vec<ModpackVersionEntry>,
    current_id: &str,
) -> ModpackUpdateStatus {
    match crate::commands::latest_newer(versions, current_id) {
        Some(entry) => ModpackUpdateStatus::UpdateAvailable { entry },
        None => ModpackUpdateStatus::UpToDate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ver(id: &str, date: &str) -> ModpackVersionEntry {
        ModpackVersionEntry {
            id: id.into(),
            name: id.into(),
            version_number: id.into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            date_published: date.into(),
        }
    }

    #[test]
    fn precheck_not_a_pack_when_source_missing() {
        let r = precheck(None, Some("p"), Some("v"), false);
        assert_eq!(
            *r.unwrap_err(),
            ModpackUpdateStatus::NotCheckable {
                reason: NotCheckableReason::NotAPack
            }
        );
    }

    #[test]
    fn precheck_no_provenance_when_version_id_missing() {
        let r = precheck(Some(ModSource::Modrinth), Some("p"), None, false);
        assert_eq!(
            *r.unwrap_err(),
            ModpackUpdateStatus::NotCheckable {
                reason: NotCheckableReason::NoProvenance
            }
        );
    }

    #[test]
    fn precheck_no_provenance_when_project_id_missing() {
        let r = precheck(Some(ModSource::Modrinth), None, Some("v"), false);
        assert_eq!(
            *r.unwrap_err(),
            ModpackUpdateStatus::NotCheckable {
                reason: NotCheckableReason::NoProvenance
            }
        );
    }

    #[test]
    fn precheck_needs_cf_key_when_curseforge_and_no_key() {
        let r = precheck(Some(ModSource::Curseforge), Some("p"), Some("v"), false);
        assert_eq!(
            *r.unwrap_err(),
            ModpackUpdateStatus::NotCheckable {
                reason: NotCheckableReason::NeedsCurseforgeKey
            }
        );
    }

    #[test]
    fn precheck_curseforge_proceeds_with_key() {
        let r = precheck(Some(ModSource::Curseforge), Some("p"), Some("v"), true);
        assert_eq!(r.unwrap(), (ModSource::Curseforge, "p".into(), "v".into()));
    }

    #[test]
    fn precheck_non_cf_proceeds_ignoring_cf_key() {
        // Non-CF sources never require a key, regardless of the flag.
        let r = precheck(Some(ModSource::Ftb), Some("p"), Some("v"), false);
        assert_eq!(r.unwrap(), (ModSource::Ftb, "p".into(), "v".into()));
    }

    #[test]
    fn status_up_to_date_when_current_is_newest() {
        let list = vec![
            ver("v2", "2026-03-01T00:00:00Z"),
            ver("v1", "2026-01-01T00:00:00Z"),
        ];
        assert_eq!(
            status_from_versions(list, "v2"),
            ModpackUpdateStatus::UpToDate
        );
    }

    #[test]
    fn status_available_when_newer_exists() {
        let list = vec![
            ver("v2", "2026-03-01T00:00:00Z"),
            ver("v1", "2026-01-01T00:00:00Z"),
        ];
        match status_from_versions(list, "v1") {
            ModpackUpdateStatus::UpdateAvailable { entry } => assert_eq!(entry.id, "v2"),
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
    }

    #[test]
    fn status_up_to_date_when_list_empty() {
        assert_eq!(
            status_from_versions(vec![], "v1"),
            ModpackUpdateStatus::UpToDate
        );
    }
}
