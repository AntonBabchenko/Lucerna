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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    #[serde(rename = "neoforge")]
    NeoForge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceFile {
    pub version: u32,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct AppFile {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_instance: Option<String>,
    #[serde(default)]
    pub onboarding: OnboardingState,
}

impl Default for AppFile {
    fn default() -> Self {
        Self {
            version: 1,
            active_instance: None,
            onboarding: OnboardingState::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct OnboardingState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tour_completed_version: Option<String>,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InstanceFile {
        InstanceFile {
            version: 1,
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
    fn app_file_default_is_v1_with_no_active() {
        let app = AppFile::default();
        assert_eq!(app.version, 1);
        assert_eq!(app.active_instance, None);
        let json = serde_json::to_string(&app).unwrap();
        let back: AppFile = serde_json::from_str(&json).unwrap();
        assert_eq!(app, back);
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
        assert!(json.contains(r#""loader_version":"49.0.49""#), "got: {json}");
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
        assert!(json.contains(r#""loader_version":"20.4.245""#), "got: {json}");
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
        assert!(json.contains(r#""mrpack_name":"All The Mods 10""#), "got: {json}");
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
}
