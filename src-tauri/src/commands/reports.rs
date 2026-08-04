use super::*;

// =========================================================================
// Install reports — read a persisted per-task report (see `crate::reports`)
// =========================================================================

/// Read a previously persisted install/import report for `task_id`, in full.
///
/// No paging contract here, unlike `instance_journal_read`: the journal is a
/// single growing jsonl file that a page size makes sense against, but a
/// report is one small, already-bounded JSON file (rotation caps the total
/// on disk, not any one file's row count), and the History UI wants every
/// row at once so it can filter and virtualize rendering client-side.
///
/// `Ok(None)` when the task never produced a report, or its report already
/// rotated out — not an error, since both are ordinary states for an
/// instance with a long history.
#[tauri::command]
#[specta::specta]
pub fn instance_report_read(
    app: tauri::AppHandle,
    instance_id: String,
    task_id: String,
) -> Result<Option<crate::reports::TaskReport>, crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::reports::read(&inst_root, &task_id)
}
