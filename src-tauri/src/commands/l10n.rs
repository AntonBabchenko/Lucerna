use super::*;

// =========================================================================
// In-game mod localization — translation coverage
// =========================================================================

pub use crate::l10n::coverage::InstanceCoverage;

/// Scan the instance's enabled mods for language coverage in `lang`.
///
/// Per-jar results are cached by (language, jar SHA-1), so an unchanged
/// instance re-renders with zero jar reads. An empty `lang` means "derive from
/// the UI locale": the persisted `GeneralSettings.language` from `app.json`,
/// mapped through `default_target_code`.
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
        ui_locale_target_code(&app)
    } else {
        lang
    };
    crate::l10n::coverage::scan_instance(&inst_root, &cache_path, &store_dir, &lang).await
}

/// Resolve the launcher's UI locale to a Minecraft target code, for the
/// empty-`lang` "derive from the UI locale" case.
///
/// A settings read failure here — missing `app.json` on a fresh install, a
/// corrupt file, an unreadable data root — must not fail the coverage scan:
/// the whole point of this fallback is to pick a REASONABLE default language,
/// and the language picker in the UI lets the user override it either way.
/// `"en_us"` is always a safe value to fall back to.
fn ui_locale_target_code(app: &tauri::AppHandle) -> String {
    let Ok(path) = crate::paths::app_file(app) else {
        return "en_us".to_string();
    };
    let Ok(settings) = crate::instances::store::read_app_json(&path) else {
        return "en_us".to_string();
    };
    resolve_ui_language(&settings.general.language)
}

/// Map the raw `GeneralSettings.language` value (`"system"`, or a BCP-47-ish
/// code such as `"ru"` / `"pt-BR"`) to a Minecraft target code. Split out as a
/// pure function — unlike the app.json read around it — so the `"system"`
/// special-case is unit-testable without a Tauri handle.
fn resolve_ui_language(language: &str) -> String {
    if language == "system" {
        return "en_us".to_string();
    }
    crate::l10n::coverage::default_target_code(language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_language_setting_falls_back_to_english() {
        assert_eq!(resolve_ui_language("system"), "en_us");
    }

    #[test]
    fn a_real_language_setting_is_mapped_through_default_target_code() {
        assert_eq!(resolve_ui_language("ru"), "ru_ru");
        assert_eq!(resolve_ui_language("pt-BR"), "pt_br");
    }
}
