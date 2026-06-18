//! On-disk and IPC types for instances.
//!
//! `InstanceFile` is the disk shape (read/written by `instances::store`).
//! `InstanceWithStatus` is the IPC shape returned to the UI — `InstanceFile`
//! flattened + a precomputed `ready` boolean (so the dropdown doesn't make
//! N filesystem checks per render).
//!
//! `AppFile` lives at `<app_data_dir>/app.json`. Holds the active-instance
//! pointer and onboarding state. Extend here for further app-level prefs
//! (Simple Mode toggle, UI prefs) without touching `account.json` or each
//! `instance.json`.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Which third-party launcher an instance was imported from. Distinct
/// from `ModSource` (a mod *platform*) — this is the *launcher* of origin.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForeignLauncher {
    Prism,
    CurseforgeApp,
    ModrinthApp,
    Atlauncher,
    RawMinecraft,
    /// Official Mojang / Microsoft launcher (profile model).
    MojangLauncher,
    /// TLauncher (profile model; detected via marker files).
    Tlauncher,
}

/// Provenance written when an instance is created via launcher import.
/// `None` for manually-created and modpack-imported instances.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ImportProvenance {
    pub launcher: ForeignLauncher,
    pub source_name: String,
    pub source_path: String,
    /// f64 to satisfy specta-typescript (no u64); within JS safe-int range.
    pub imported_unix_ms: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    #[serde(rename = "neoforge")]
    NeoForge,
}

impl LoaderKind {
    /// The loader's Modrinth slug, as it appears in a version object's
    /// `loaders` array and the search `loaders` facet.
    pub(crate) fn modrinth_slug(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
            LoaderKind::Vanilla => "minecraft",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceFile {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: u32,
    pub extra_jvm_args: String,
    /// f64 because specta-typescript 0.0.12 forbids u64. JS `Date.now()`
    /// values round-trip cleanly within the 2^53 safe-integer range.
    pub created_unix_ms: f64,
    /// Origin pack display name when this instance was created via
    /// modpack import. `None` for manually-created instances.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrpack_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrpack_version: Option<String>,
    /// Project id on the source platform (Modrinth project_id, or the
    /// CurseForge mod id formatted as a string). Lets the Imported view
    /// link back to the pack's page on its source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrpack_project_id: Option<String>,
    /// Which platform the pack was sourced from. `None` for manually-
    /// created instances; set to `Modrinth` or `Curseforge` on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrpack_source: Option<crate::mods::platform::ModSource>,
    /// Short description fetched from the source platform at import
    /// time. `None` when the lookup failed (best-effort) or the source
    /// is CurseForge (no summary backfill implemented).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrpack_summary: Option<String>,
    /// Modrinth version id (opaque, e.g. `vyRB9jtS`) of the pack version
    /// this instance is currently on. Set on Browse-flow imports (the
    /// version drawer knows it); `None` for drag-drop imports and
    /// manually-created instances. Used by the update flow to compare
    /// reliably against the Modrinth API (the human `mrpack_version`
    /// string is not a stable identifier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrpack_version_id: Option<String>,
    /// Summary of the instance's last Verify/Repair integrity check.
    /// `None` until the user runs Verify once. Additive — old instance.json
    /// without it deserialises to None (no schema-version bump).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<crate::verify::IntegrityStatus>,
    /// Set when this instance was imported from another launcher.
    /// Additive — old instance.json without it deserialises to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<ImportProvenance>,
    /// Signature (see `logs::files::log_signature`) of the latest diagnosable
    /// log at the moment the user last applied a repair. While the latest log
    /// still matches this, an otherwise-unverifiable diagnosis is shown as
    /// "handled". Additive — old instance.json deserialises to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handled_log_sig: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct AppFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_instance: Option<String>,
    #[serde(default)]
    pub onboarding: OnboardingState,
    #[serde(default)]
    pub general: GeneralSettings,
    /// The latest version the user explicitly dismissed from the
    /// update toast. Suppresses re-notifying for that same version; a
    /// newer release clears the suppression naturally (version differs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_dismissed_version: Option<String>,
}

impl Default for AppFile {
    fn default() -> Self {
        Self {
            active_instance: None,
            onboarding: OnboardingState::default(),
            general: GeneralSettings::default(),
            update_dismissed_version: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct OnboardingState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tour_completed_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// Which GPU Minecraft should prefer. OS-neutral: Windows maps
/// `HighPerformance→GpuPreference=2 / PowerSaving→1 / Auto→absent`;
/// Linux maps `HighPerformance→PRIME/DRI offload / {PowerSaving,Auto}→none`.
/// macOS ignores it (no mechanism). Default `Auto` = today's behavior.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuPreference {
    #[default]
    Auto,
    HighPerformance,
    PowerSaving,
}

/// How verbose onboarding/help copy is. `Basic` = plain language (default,
/// understandable to newcomers); `Advanced` = the original technical copy.
/// Chosen on first launch and changeable in Settings → General.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum ExplanationLevel {
    #[default]
    Basic,
    Advanced,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "system".to_string()
}

/// Opt-in automatic cleanup of old log files. Applied per-instance on
/// game exit and when the Logs window opens. `latest.log` and
/// `debug.log` are always preserved. Two limits, both enforced: keep at
/// most `max_files` non-protected files AND keep their total size under
/// `max_total_mb`. Off by default — the launcher never deletes user logs
/// without explicit opt-in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct LogRetentionPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_files")]
    pub max_files: u32,
    #[serde(default = "default_max_total_mb")]
    pub max_total_mb: u32,
}

fn default_max_files() -> u32 {
    10
}

fn default_max_total_mb() -> u32 {
    100
}

impl Default for LogRetentionPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_files: default_max_files(),
            max_total_mb: default_max_total_mb(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct GeneralSettings {
    /// When true, the launcher window hides to a system-tray icon on
    /// MC spawn and auto-restores on MC exit. Default false — opt-in
    /// via Settings → General.
    #[serde(default)]
    pub hide_to_tray_during_game: bool,
    /// UI theme preference: system (follow OS), light, or dark.
    /// Default system — user can override via Settings → General.
    #[serde(default)]
    pub theme: ThemePreference,
    /// When true (default), the launcher checks GitHub Releases on
    /// startup and shows a sticky toast if a newer version exists. The
    /// install is always an explicit click — this only gates the check
    /// and the notification. Opt-out via Settings → General.
    #[serde(default = "default_true")]
    pub check_updates_on_startup: bool,
    /// UI language preference. `"system"` (follow OS) or a BCP-47 code
    /// such as `"en"` / `"ru"`. Stored as an opaque string so community
    /// translations need no Rust change — the frontend validates and
    /// falls back. Default `"system"`.
    #[serde(default = "default_language")]
    pub language: String,
    /// Verbosity of onboarding/help copy. `#[serde(default)]` → existing
    /// app.json files (written before this field existed) deserialize to
    /// `Basic`, matching the chosen default for upgraders.
    #[serde(default)]
    pub explanation_level: ExplanationLevel,
    /// When true, the launcher starts in (and is currently in) compact /
    /// mini launch-pad mode: the right content column is hidden and the OS
    /// window is shrunk to the sidebar strip. Default false. Updated on
    /// every compact/expand toggle.
    #[serde(default)]
    pub compact_mode: bool,
    /// Preferred GPU for the Minecraft process. `#[serde(default)]` →
    /// app.json written before this field deserializes to `Auto`.
    #[serde(default)]
    pub gpu_preference: GpuPreference,
    /// Opt-in old-log auto-cleanup. `#[serde(default)]` → app.json
    /// written before this field deserializes to a disabled policy.
    #[serde(default)]
    pub log_retention: LogRetentionPolicy,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            hide_to_tray_during_game: false,
            theme: ThemePreference::default(),
            check_updates_on_startup: true,
            language: default_language(),
            explanation_level: ExplanationLevel::default(),
            compact_mode: false,
            gpu_preference: GpuPreference::default(),
            log_retention: LogRetentionPolicy::default(),
        }
    }
}

/// What the UI sees per row in the instance dropdown.
#[derive(Debug, Clone, Serialize, Type)]
pub struct InstanceWithStatus {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: u32,
    pub extra_jvm_args: String,
    pub created_unix_ms: f64,
    /// True iff the effective version JAR is on disk. UI shows ✓/↓ icon.
    pub ready: bool,
    pub mrpack_name: Option<String>,
    pub mrpack_version: Option<String>,
    pub mrpack_project_id: Option<String>,
    pub mrpack_source: Option<crate::mods::platform::ModSource>,
    pub mrpack_summary: Option<String>,
    pub mrpack_version_id: Option<String>,
    pub integrity: Option<crate::verify::IntegrityStatus>,
    pub imported_from: Option<ImportProvenance>,
}

impl InstanceWithStatus {
    pub fn from_file(file: &InstanceFile, ready: bool) -> Self {
        Self {
            id: file.id.clone(),
            name: file.name.clone(),
            mc_version: file.mc_version.clone(),
            loader: file.loader,
            loader_version: file.loader_version.clone(),
            max_heap_mb: file.max_heap_mb,
            extra_jvm_args: file.extra_jvm_args.clone(),
            created_unix_ms: file.created_unix_ms,
            ready,
            mrpack_name: file.mrpack_name.clone(),
            mrpack_version: file.mrpack_version.clone(),
            mrpack_project_id: file.mrpack_project_id.clone(),
            mrpack_source: file.mrpack_source,
            mrpack_summary: file.mrpack_summary.clone(),
            mrpack_version_id: file.mrpack_version_id.clone(),
            integrity: file.integrity.clone(),
            imported_from: file.imported_from.clone(),
        }
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    #[test]
    fn log_retention_default_is_off_with_sane_numbers() {
        let p = LogRetentionPolicy::default();
        assert!(!p.enabled, "retention must be opt-in (off by default)");
        assert_eq!(p.max_files, 10);
        assert_eq!(p.max_total_mb, 100);
    }

    #[test]
    fn general_settings_default_has_retention_off() {
        let g = GeneralSettings::default();
        assert!(!g.log_retention.enabled);
    }

    #[test]
    fn old_app_json_without_retention_deserializes_to_default() {
        // Field added later → existing app.json files lack it. #[serde(default)]
        // must fill it in rather than fail the whole GeneralSettings parse.
        let json = r#"{"hide_to_tray_during_game":true}"#;
        let g: GeneralSettings = serde_json::from_str(json).unwrap();
        assert!(!g.log_retention.enabled);
        assert_eq!(g.log_retention.max_files, 10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InstanceFile {
        InstanceFile {
            id: "3f4a-bbbb-cccc-dddd-eeeeffffaaaa".into(),
            name: "Default".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from: None,
            handled_log_sig: None,
        }
    }

    #[test]
    fn instance_file_roundtrip() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn loader_kind_serializes_snake_case() {
        let json = serde_json::to_string(&LoaderKind::Fabric).unwrap();
        assert_eq!(json, r#""fabric""#);
        let json = serde_json::to_string(&LoaderKind::Vanilla).unwrap();
        assert_eq!(json, r#""vanilla""#);
        let json = serde_json::to_string(&LoaderKind::Quilt).unwrap();
        assert_eq!(json, r#""quilt""#);
    }

    #[test]
    fn fabric_with_loader_version_roundtrip() {
        let mut s = sample();
        s.loader = LoaderKind::Fabric;
        s.loader_version = Some("0.16.5".into());
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""loader":"fabric""#), "got: {json}");
        assert!(json.contains(r#""loader_version":"0.16.5""#), "got: {json}");
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn fresh_install_state_empty_mc_version() {
        let mut s = sample();
        s.mc_version = String::new();
        let json = serde_json::to_string(&s).unwrap();
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mc_version, "");
    }

    #[test]
    fn app_file_default_has_no_active_and_roundtrips() {
        let app = AppFile::default();
        assert_eq!(app.active_instance, None);
        let json = serde_json::to_string(&app).unwrap();
        let back: AppFile = serde_json::from_str(&json).unwrap();
        assert_eq!(app, back);
    }

    #[test]
    fn handled_log_sig_defaults_to_none_for_old_json() {
        // An instance.json written before this field existed must still parse.
        let json = r#"{
            "id": "abc", "name": "X", "mc_version": "1.20.1",
            "loader": "vanilla", "loader_version": null, "max_heap_mb": 2048,
            "extra_jvm_args": "", "created_unix_ms": 0
        }"#;
        let f: InstanceFile = serde_json::from_str(json).unwrap();
        assert_eq!(f.handled_log_sig, None);
    }

    #[test]
    fn instance_with_status_from_file_preserves_fields() {
        let s = sample();
        let w = InstanceWithStatus::from_file(&s, true);
        assert_eq!(w.id, s.id);
        assert_eq!(w.mc_version, s.mc_version);
        assert_eq!(w.loader, s.loader);
        assert!(w.ready);
    }

    #[test]
    fn loader_kind_serializes_forge_as_snake_case() {
        let json = serde_json::to_string(&LoaderKind::Forge).unwrap();
        assert_eq!(json, r#""forge""#);
    }

    #[test]
    fn forge_with_loader_version_roundtrip() {
        let mut s = sample();
        s.loader = LoaderKind::Forge;
        s.loader_version = Some("49.0.49".into());
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""loader":"forge""#), "got: {json}");
        assert!(
            json.contains(r#""loader_version":"49.0.49""#),
            "got: {json}"
        );
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn loader_kind_serializes_neoforge_as_neoforge() {
        let json = serde_json::to_string(&LoaderKind::NeoForge).unwrap();
        assert_eq!(json, r#""neoforge""#);
    }

    #[test]
    fn neoforge_with_loader_version_roundtrip() {
        let mut s = sample();
        s.loader = LoaderKind::NeoForge;
        s.loader_version = Some("20.4.245".into());
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""loader":"neoforge""#), "got: {json}");
        assert!(
            json.contains(r#""loader_version":"20.4.245""#),
            "got: {json}"
        );
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn instance_file_deserializes_old_json_with_no_mrpack_fields() {
        let json = r#"{
            "version": 1,
            "id": "abc",
            "name": "Old",
            "mc_version": "1.20.1",
            "loader": "vanilla",
            "loader_version": null,
            "max_heap_mb": 2048,
            "extra_jvm_args": "",
            "created_unix_ms": 1700000000000.0
        }"#;
        let inst: InstanceFile = serde_json::from_str(json).unwrap();
        assert_eq!(inst.mrpack_name, None);
        assert_eq!(inst.mrpack_version, None);
    }

    #[test]
    fn instance_file_serializes_skip_none_mrpack_fields() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("mrpack_name"), "got: {json}");
        assert!(!json.contains("mrpack_version"), "got: {json}");
    }

    #[test]
    fn instance_file_roundtrips_with_some_mrpack_fields() {
        let mut s = sample();
        s.mrpack_name = Some("All The Mods 10".into());
        s.mrpack_version = Some("1.4.7".into());
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains(r#""mrpack_name":"All The Mods 10""#),
            "got: {json}"
        );
        assert!(json.contains(r#""mrpack_version":"1.4.7""#), "got: {json}");
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn instance_with_status_carries_mrpack_fields() {
        let mut s = sample();
        s.mrpack_name = Some("Fabulously Optimized".into());
        s.mrpack_version = Some("5.9.0".into());
        let w = InstanceWithStatus::from_file(&s, true);
        assert_eq!(w.mrpack_name.as_deref(), Some("Fabulously Optimized"));
        assert_eq!(w.mrpack_version.as_deref(), Some("5.9.0"));
    }

    #[test]
    fn instance_file_serializes_skip_none_new_mrpack_fields() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("mrpack_project_id"), "got: {json}");
        assert!(!json.contains("mrpack_source"), "got: {json}");
        assert!(!json.contains("mrpack_summary"), "got: {json}");
    }

    #[test]
    fn instance_file_roundtrips_with_full_mrpack_metadata() {
        let mut s = sample();
        s.mrpack_name = Some("X".into());
        s.mrpack_version = Some("1.0".into());
        s.mrpack_project_id = Some("ABCD1234".into());
        s.mrpack_source = Some(crate::mods::platform::ModSource::Modrinth);
        s.mrpack_summary = Some("A pack".into());
        let json = serde_json::to_string(&s).unwrap();
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn app_file_default_general_settings_are_off() {
        let app = AppFile::default();
        assert!(!app.general.hide_to_tray_during_game);
    }

    #[test]
    fn app_file_round_trips_general_block() {
        let mut app = AppFile::default();
        app.general.hide_to_tray_during_game = true;
        let json = serde_json::to_string(&app).unwrap();
        let back: AppFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, app);
    }

    #[test]
    fn app_file_parses_old_json_without_general_block() {
        // Real on-disk shape from a v1 install — no `general` field.
        let old_json = r#"{
            "version": 1,
            "active_instance": "abc",
            "onboarding": { "tour_completed_version": "0.5.0" }
        }"#;
        let parsed: AppFile = serde_json::from_str(old_json).unwrap();
        assert!(!parsed.general.hide_to_tray_during_game);
        assert_eq!(parsed.active_instance.as_deref(), Some("abc"));
    }

    #[test]
    fn theme_preference_default_is_system() {
        let pref = ThemePreference::default();
        assert_eq!(pref, ThemePreference::System);
    }

    #[test]
    fn general_settings_default_theme_is_system() {
        let gs = GeneralSettings::default();
        assert_eq!(gs.theme, ThemePreference::System);
    }

    #[test]
    fn theme_preference_serde_round_trip() {
        for pref in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            let json = serde_json::to_string(&pref).unwrap();
            let back: ThemePreference = serde_json::from_str(&json).unwrap();
            assert_eq!(back, pref);
        }
    }

    #[test]
    fn theme_preference_serializes_lowercase() {
        let json = serde_json::to_string(&ThemePreference::Light).unwrap();
        assert_eq!(json, r#""light""#);
    }

    #[test]
    fn app_file_parses_old_general_block_without_theme() {
        let old_json = r#"{
            "version": 1,
            "active_instance": null,
            "onboarding": { "tour_completed_version": null },
            "general": { "hide_to_tray_during_game": true }
        }"#;
        let parsed: AppFile = serde_json::from_str(old_json).unwrap();
        assert!(parsed.general.hide_to_tray_during_game);
        assert_eq!(parsed.general.theme, ThemePreference::System);
    }

    #[test]
    fn general_settings_defaults_check_updates_on() {
        let g = GeneralSettings::default();
        assert!(
            g.check_updates_on_startup,
            "updates check should default on (opt-out)"
        );
    }

    #[test]
    fn app_json_missing_update_field_defaults_check_on() {
        // An app.json from before this field existed must deserialize with
        // the check enabled (opt-out), not disabled.
        let json = r#"{ "version": 1, "general": { "hide_to_tray_during_game": false } }"#;
        let parsed: AppFile = serde_json::from_str(json).unwrap();
        assert!(parsed.general.check_updates_on_startup);
        assert_eq!(parsed.update_dismissed_version, None);
    }

    #[test]
    fn general_settings_defaults_explanation_level_to_basic() {
        let g = GeneralSettings::default();
        assert_eq!(g.explanation_level, ExplanationLevel::Basic);
    }

    #[test]
    fn app_file_without_explanation_level_deserializes_to_basic() {
        // app.json written before the field existed (general present, no level).
        let old_json = r#"{
            "version": 1,
            "active_instance": null,
            "general": { "hide_to_tray_during_game": true }
        }"#;
        let parsed: AppFile = serde_json::from_str(old_json).unwrap();
        assert_eq!(parsed.general.explanation_level, ExplanationLevel::Basic);
    }

    #[test]
    fn general_settings_roundtrips_explanation_level_advanced() {
        let mut g = GeneralSettings::default();
        g.explanation_level = ExplanationLevel::Advanced;
        let json = serde_json::to_string(&g).unwrap();
        let back: GeneralSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.explanation_level, ExplanationLevel::Advanced);
    }

    #[test]
    fn general_settings_default_compact_mode_is_off() {
        let gs = GeneralSettings::default();
        assert!(!gs.compact_mode, "compact mode should default off");
    }

    #[test]
    fn app_file_round_trips_compact_mode() {
        let mut app = AppFile::default();
        app.general.compact_mode = true;
        let json = serde_json::to_string(&app).unwrap();
        let back: AppFile = serde_json::from_str(&json).unwrap();
        assert!(back.general.compact_mode);
        assert_eq!(back, app);
    }

    #[test]
    fn app_file_without_compact_mode_deserializes_to_off() {
        // app.json written before the field existed (general present, no field).
        let old_json = r#"{
            "version": 1,
            "active_instance": null,
            "general": { "hide_to_tray_during_game": true }
        }"#;
        let parsed: AppFile = serde_json::from_str(old_json).unwrap();
        assert!(!parsed.general.compact_mode);
    }

    #[test]
    fn gpu_preference_default_is_auto() {
        assert_eq!(GpuPreference::default(), GpuPreference::Auto);
    }

    #[test]
    fn gpu_preference_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&GpuPreference::HighPerformance).unwrap(),
            r#""high_performance""#
        );
        assert_eq!(
            serde_json::to_string(&GpuPreference::PowerSaving).unwrap(),
            r#""power_saving""#
        );
        assert_eq!(
            serde_json::to_string(&GpuPreference::Auto).unwrap(),
            r#""auto""#
        );
    }

    #[test]
    fn general_settings_default_gpu_pref_is_auto() {
        assert_eq!(
            GeneralSettings::default().gpu_preference,
            GpuPreference::Auto
        );
    }

    #[test]
    fn app_file_without_gpu_pref_deserializes_to_auto() {
        let old = r#"{ "version": 1, "general": { "hide_to_tray_during_game": true } }"#;
        let parsed: AppFile = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.general.gpu_preference, GpuPreference::Auto);
    }

    #[test]
    fn general_settings_roundtrips_gpu_pref() {
        let mut g = GeneralSettings::default();
        g.gpu_preference = GpuPreference::HighPerformance;
        let back: GeneralSettings =
            serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back.gpu_preference, GpuPreference::HighPerformance);
    }

    #[test]
    fn app_json_roundtrips_dismissed_version() {
        let mut f = AppFile::default();
        f.update_dismissed_version = Some("0.9.1".into());
        let s = serde_json::to_string(&f).unwrap();
        let back: AppFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.update_dismissed_version, Some("0.9.1".into()));
    }

    fn sample_integrity() -> crate::verify::IntegrityStatus {
        crate::verify::IntegrityStatus {
            healthy: false,
            checked_unix_ms: 1_700_000_000_000.0,
            categories: vec![crate::verify::CategoryReport {
                category: crate::verify::VerifyCategory::Assets,
                total: 5,
                ok: 3,
                missing: 2,
                corrupt: 0,
            }],
            problem_count: 2,
        }
    }

    #[test]
    fn instance_file_serializes_skip_none_integrity() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("integrity"), "got: {json}");
    }

    #[test]
    fn instance_file_roundtrips_with_some_integrity() {
        let mut s = sample();
        s.integrity = Some(sample_integrity());
        let json = serde_json::to_string(&s).unwrap();
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.integrity, Some(sample_integrity()));
    }

    #[test]
    fn instance_file_deserializes_old_json_with_no_integrity_field() {
        let json = r#"{
            "version": 1,
            "id": "abc",
            "name": "Old",
            "mc_version": "1.20.1",
            "loader": "vanilla",
            "loader_version": null,
            "max_heap_mb": 2048,
            "extra_jvm_args": "",
            "created_unix_ms": 1700000000000.0
        }"#;
        let inst: InstanceFile = serde_json::from_str(json).unwrap();
        assert_eq!(inst.integrity, None);
    }

    #[test]
    fn instance_with_status_carries_integrity() {
        let mut s = sample();
        s.integrity = Some(sample_integrity());
        let w = InstanceWithStatus::from_file(&s, true);
        assert_eq!(w.integrity, Some(sample_integrity()));
    }

    #[test]
    fn instance_file_roundtrips_with_imported_from() {
        let mut s = sample();
        s.imported_from = Some(ImportProvenance {
            launcher: ForeignLauncher::Prism,
            source_name: "ATM9".into(),
            source_path: r"C:\Users\x\AppData\Roaming\PrismLauncher\instances\ATM9".into(),
            imported_unix_ms: 1_700_000_000_000.0,
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn instance_file_serializes_skip_none_imported_from() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("imported_from"), "got: {json}");
    }

    #[test]
    fn foreign_launcher_serializes_new_variants_snake_case() {
        use super::ForeignLauncher;
        assert_eq!(
            serde_json::to_string(&ForeignLauncher::MojangLauncher).unwrap(),
            "\"mojang_launcher\""
        );
        assert_eq!(
            serde_json::to_string(&ForeignLauncher::Tlauncher).unwrap(),
            "\"tlauncher\""
        );
    }

    #[test]
    fn instance_file_deserializes_old_json_without_imported_from() {
        let json = r#"{
            "version": 1, "id": "abc", "name": "Old", "mc_version": "1.20.1",
            "loader": "vanilla", "loader_version": null, "max_heap_mb": 2048,
            "extra_jvm_args": "", "created_unix_ms": 1700000000000.0
        }"#;
        let inst: InstanceFile = serde_json::from_str(json).unwrap();
        assert_eq!(inst.imported_from, None);
    }
}
