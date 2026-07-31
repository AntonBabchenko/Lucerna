use super::*;

// =========================================================================
// In-game mod localization — translation coverage
// =========================================================================

pub use crate::l10n::coverage::InstanceCoverage;

/// Scan the instance's enabled mods for language coverage in `lang`.
///
/// Per-jar results are cached by (language, jar SHA-1), so an unchanged
/// instance re-renders with zero jar reads. An empty `lang` means "derive from
/// the UI locale", which the caller passes through from the i18n store.
#[tauri::command]
#[specta::specta]
pub async fn l10n_coverage(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
) -> Result<InstanceCoverage, crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    let cache_path = crate::paths::l10n_scan_cache_file(&app)
        .map_err(|e| crate::error::Error::io("<l10n_scan_cache_file>", e))?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let lang = if lang.is_empty() {
        crate::l10n::coverage::default_target_code("en")
    } else {
        lang
    };
    crate::l10n::coverage::scan_instance(&inst_root, &cache_path, &store_dir, &lang).await
}
