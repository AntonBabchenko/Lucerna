use super::*;

// =========================================================================
// Instance journal — read + clear the per-instance activity history
// =========================================================================

/// Display name + version of an installed mod, for a journal row keyed only by
/// SHA-1 (enable / disable / uninstall take nothing else).
///
/// Callers on a removal path MUST call this BEFORE the mutation — once the
/// registry row is gone there is no name left to record. Returns `None` when
/// the sha is unknown or the registry can't be read; the journal row then
/// falls back to an empty subject rather than blocking the operation.
pub(crate) async fn mod_identity(
    inst_root: &std::path::Path,
    sha1: &str,
) -> Option<(String, Option<String>)> {
    let installed = crate::mods::installed::list(inst_root).await.ok()?;
    installed
        .iter()
        .find(|m| m.sha1.eq_ignore_ascii_case(sha1))
        .map(|m| (m.name.clone(), m.version_number.clone()))
}

/// Newest-first slice of the instance's journal. `limit` is clamped to
/// `[1, MAX_ENTRIES]`; `0` means "the caller has no opinion" and maps to
/// the default page size. An instance with no recorded activity returns an
/// empty list, not an error.
#[tauri::command]
#[specta::specta]
pub fn instance_journal_read(
    app: tauri::AppHandle,
    instance_id: String,
    limit: u32,
) -> Result<Vec<crate::journal::JournalEntry>, crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    let limit = if limit == 0 {
        crate::journal::DEFAULT_READ_LIMIT
    } else {
        (limit as usize).min(crate::journal::MAX_ENTRIES)
    };
    crate::journal::read(&inst_root, limit)
}

/// Delete the instance's journal. The history is the user's own local data,
/// so they get an explicit way to drop it (mirrors "clear old logs"). Absent
/// journal is a no-op.
#[tauri::command]
#[specta::specta]
pub fn instance_journal_clear(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::journal::clear(&inst_root)
}
