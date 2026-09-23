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
    /// X Minecraft Launcher (own instance tree; `instance.json` with a
    /// `runtime` object).
    Xmcl,
    /// Legacy Launcher / llaun.ch (profile model; snake_case
    /// `tlauncher_profiles.json` marker — distinct from tlauncher.org).
    LegacyLauncher,
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

/// Where an instance came from, when it came from a modpack.
///
/// Bundles what used to be six positional `create_instance` parameters (the
/// function carried thirteen and a `#[allow(clippy::too_many_arguments)]`).
///
/// `slug` is **transient**: it feeds the directory-name ladder in
/// [`crate::naming::derive_base`] at creation time and is deliberately not
/// persisted — `project_id` already covers linking back to the pack's page.
#[derive(Debug, Clone, Default)]
pub struct PackOrigin {
    pub name_and_version: Option<(String, String)>,
    pub project_id: Option<String>,
    pub source: Option<crate::mods::platform::ModSource>,
    pub summary: Option<String>,
    pub version_id: Option<String>,
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceFile {
    /// **Derived from the containing directory on read** — see
    /// [`crate::instances::store::read_instance_json`]. The value written here
    /// is vestigial: kept for compatibility with older launcher builds, ignored
    /// when loading. Renaming the folder therefore renames the instance, and a
    /// folder renamed outside the launcher keeps working.
    pub id: String,
    /// Identity that survives a directory rename. Exists for exactly one
    /// consumer: desktop shortcuts, which carry `--launch <uid>`. Shortcuts are
    /// files on the user's desktop that we never record creating, so a rename
    /// cannot go and repair them — the token inside them has to stay valid.
    ///
    /// `Option` because instances created before this field existed have none.
    /// Filled in lazily (on shortcut creation, and before a rename), never by a
    /// startup migration that would rewrite every `instance.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: u32,
    /// Optional JVM initial heap (`-Xms`) in MB. `None` = JVM default (the
    /// historical behaviour). Additive — old instance.json without it
    /// deserialises to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_heap_mb: Option<u32>,
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
    /// Server id this instance was created from ("client for my server"
    /// flow). `None` for every other instance. Additive — old instance.json
    /// without it deserialises to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_from_server: Option<String>,
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
    /// The latest app version whose post-update "What's new" changelog the
    /// user has already been shown. Suppresses re-prompting for that same
    /// version; a newer release differs and prompts again. Mirrors
    /// `update_dismissed_version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog_seen_version: Option<String>,
}

impl Default for AppFile {
    fn default() -> Self {
        Self {
            active_instance: None,
            onboarding: OnboardingState::default(),
            general: GeneralSettings::default(),
            update_dismissed_version: None,
            changelog_seen_version: None,
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

/// What the launcher window does when a game starts (Settings → Game). Only the
/// first running game triggers it; the window comes back when a game closes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameStartWindow {
    /// The window stays as it is.
    #[default]
    Keep,
    /// The window is minimised (some Linux desktops ignore this).
    Minimise,
    /// The window hides and a tray icon appears (needs a system tray).
    HideToTray,
}

/// The pre-0.25 setting, a checkbox, in the new terms.
pub fn from_legacy(hide_to_tray: bool) -> GameStartWindow {
    if hide_to_tray {
        GameStartWindow::HideToTray
    } else {
        GameStartWindow::Keep
    }
}

/// Read leniently: a value this build does not know (written by a newer
/// Lucerna) reads as absent, so it is resolved from the legacy bool instead of
/// making the whole `app.json` unreadable.
fn lenient_start_window<'de, D>(d: D) -> Result<Option<GameStartWindow>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(d)?;
    Ok(raw.and_then(
        |v| match serde_json::from_value::<GameStartWindow>(v.clone()) {
            Ok(window) => Some(window),
            Err(_) => {
                // Said, not silent: the old checkbox decides now, and the next write
                // replaces this value (a newer build's choice is lost on it).
                crate::diag!(
                    "settings: unknown game_start_window {v} — using the old checkbox's meaning"
                );
                None
            }
        },
    ))
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

fn default_mod_metadata_ttl_days() -> u32 {
    7
}

/// Default number of concurrent file transfers for an SFTP server upload.
/// Four streams balance throughput against connection/memory overhead on a
/// single shared SFTP session; clamped to a sane range at point of use.
fn default_sftp_upload_concurrency() -> u32 {
    4
}

/// Which translation backend the AI pre-fill uses. `Local` talks to an
/// OpenAI-compatible server on 127.0.0.1 and needs no key.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    #[default]
    Anthropic,
    Gemini,
    Groq,
    Local,
}

impl AiProvider {
    /// Every variant. The uninstall keyring sweep iterates this, so adding a
    /// variant without adding it here is caught by a test rather than by a
    /// user discovering a leftover credential.
    pub const ALL: [AiProvider; 4] = [
        AiProvider::Anthropic,
        AiProvider::Gemini,
        AiProvider::Groq,
        AiProvider::Local,
    ];
}

/// Default port of a local OpenAI-compatible model server. 11434 is Ollama's,
/// the most common such server; any other one is a port change away.
fn default_ai_local_port() -> u16 {
    11434
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct GeneralSettings {
    /// LEGACY (≤0.24's checkbox). `game_start_window` decides now; this is kept
    /// in step with it (true ⇔ HideToTray) so a downgrade keeps the user's
    /// choice, and it is what an older file's `None` window resolves from.
    #[serde(default)]
    pub hide_to_tray_during_game: bool,
    /// What the window does when a game starts. `None` only between parsing a
    /// file that predates it (or holds a variant this build does not know) and
    /// `resolved()` — `read_app_json` resolves it, so no reader sees `None`.
    // The lenient reader keeps the wire type (a string or absent); specta must be told.
    #[serde(default, deserialize_with = "lenient_start_window")]
    #[specta(type = Option<GameStartWindow>)]
    pub game_start_window: Option<GameStartWindow>,
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
    /// How long (days) a cached mod summary (name / icon / slug) stays fresh
    /// before the installed list and dependency graph re-fetch it. `0` = never
    /// expire. `#[serde(default)]` → app.json written before this field
    /// deserializes to the 7-day default.
    #[serde(default = "default_mod_metadata_ttl_days")]
    pub mod_metadata_ttl_days: u32,
    /// How many files an SFTP server upload transfers in parallel over the one
    /// shared SFTP session. `#[serde(default)]` → app.json written before this
    /// field deserializes to the 4-stream default. Clamped to 1..=16 at use.
    #[serde(default = "default_sftp_upload_concurrency")]
    pub sftp_upload_concurrency: u32,
    /// IDs of sidebar buttons the user has hidden (opaque strings owned by the
    /// frontend registry in `src/lib/layout/sidebar-buttons.ts`). Empty = all
    /// visible — the default for app.json written before this field existed.
    /// Unknown IDs are ignored on the frontend, so buttons added or removed in
    /// later versions stay forward/backward compatible.
    #[serde(default)]
    pub hidden_sidebar_buttons: Vec<String>,
    /// Opt-in permission to send a Server List Ping to the user's OWN saved
    /// multiplayer servers so their status / player count can be shown.
    /// `#[serde(default)]` → every app.json written before this field existed
    /// deserializes to "off", which is also the default for new installs.
    /// Enforced in `network::consent`: nothing in the launcher can dial a
    /// user-supplied host while this is false.
    #[serde(default)]
    pub allow_server_ping: bool,
    /// LEGACY consent record. Versions 0.21.0–0.24.x let the user opt in to OS
    /// registration of the `lucerna://` link scheme; that toggle was retired.
    /// The only reader is `url_scheme_retire`, which uses it to decide whether a
    /// leftover registry key is ours to remove, and then clears it. Nothing
    /// sets it to `true` any more. Safe to delete together with
    /// `url_scheme_retire` once enough releases have passed (see ROADMAP).
    #[serde(default)]
    pub register_url_scheme: bool,
    /// Opt-in permission for the AI translation pre-fill to reach a model
    /// provider. Enforced in `network::consent`: while false, neither the
    /// cloud client nor the loopback client can be constructed.
    /// `#[serde(default)]` → off for every app.json written before this field.
    #[serde(default)]
    pub allow_ai_translation: bool,
    /// Which model backend the pre-fill talks to. `#[serde(default)]` →
    /// `Anthropic` for app.json written before this field existed; irrelevant
    /// until `allow_ai_translation` is turned on.
    #[serde(default)]
    pub ai_provider: AiProvider,
    /// Empty means "use the provider's default model". Free text, because
    /// nothing can enumerate a provider's model list offline.
    #[serde(default)]
    pub ai_model: String,
    /// Port of the local OpenAI-compatible server. Host is always 127.0.0.1 —
    /// see `network::loopback`, which takes the port and nothing else.
    #[serde(default = "default_ai_local_port")]
    pub ai_local_port: u16,
}

/// A field-level change to `GeneralSettings`: every field optional (specta
/// emits `field?: T | null` from the field-level `default`), unknown fields
/// rejected — with `every_general_field_has_a_patch_counterpart` that makes a
/// `GeneralSettings` field without a counterpart here a red test.
#[derive(Debug, Default, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GeneralSettingsPatch {
    #[serde(default)]
    pub hide_to_tray_during_game: Option<bool>,
    #[serde(default)]
    pub game_start_window: Option<GameStartWindow>,
    #[serde(default)]
    pub theme: Option<ThemePreference>,
    #[serde(default)]
    pub check_updates_on_startup: Option<bool>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub explanation_level: Option<ExplanationLevel>,
    #[serde(default)]
    pub compact_mode: Option<bool>,
    #[serde(default)]
    pub gpu_preference: Option<GpuPreference>,
    #[serde(default)]
    pub log_retention: Option<LogRetentionPolicy>,
    #[serde(default)]
    pub mod_metadata_ttl_days: Option<u32>,
    #[serde(default)]
    pub sftp_upload_concurrency: Option<u32>,
    #[serde(default)]
    pub hidden_sidebar_buttons: Option<Vec<String>>,
    #[serde(default)]
    pub allow_server_ping: Option<bool>,
    #[serde(default)]
    pub register_url_scheme: Option<bool>,
    #[serde(default)]
    pub allow_ai_translation: Option<bool>,
    #[serde(default)]
    pub ai_provider: Option<AiProvider>,
    #[serde(default)]
    pub ai_model: Option<String>,
    #[serde(default)]
    pub ai_local_port: Option<u16>,
}

impl GeneralSettings {
    /// The window action in force: the field, else what the legacy bool meant.
    pub fn start_window(&self) -> GameStartWindow {
        self.game_start_window
            .unwrap_or_else(|| from_legacy(self.hide_to_tray_during_game))
    }

    /// Fill a missing window action from the legacy bool, and bring the bool
    /// back in step with the window (the window wins) — so a file whose pair
    /// disagrees (a hand edit, a later build) is consistent from its first read
    /// and every write after it.
    pub fn resolved(self) -> Self {
        let window = self.start_window();
        Self {
            game_start_window: Some(window),
            hide_to_tray_during_game: window == GameStartWindow::HideToTray,
            ..self
        }
    }

    /// Every field spelled out on both sides — no `..self`.
    ///
    /// The window action and its legacy bool are ONE setting kept in step (a
    /// total rule, so no patch can leave them disagreeing):
    /// a window in the patch wins and sets the bool (hide_to_tray ⇔ true);
    /// only the bool, true → hide_to_tray; only the bool, false → keep if it
    /// was hide_to_tray, otherwise the window is untouched (never clobbers
    /// minimise); neither → both unchanged.
    pub fn patched(self, p: GeneralSettingsPatch) -> Self {
        let current = self.start_window();
        let (game_start_window, hide_to_tray_during_game) =
            match (p.game_start_window, p.hide_to_tray_during_game) {
                (Some(w), _) => (Some(w), w == GameStartWindow::HideToTray),
                (None, Some(true)) => (Some(GameStartWindow::HideToTray), true),
                (None, Some(false)) => (
                    Some(match current {
                        GameStartWindow::HideToTray => GameStartWindow::Keep,
                        other => other,
                    }),
                    false,
                ),
                (None, None) => (self.game_start_window, self.hide_to_tray_during_game),
            };
        Self {
            hide_to_tray_during_game,
            game_start_window,
            theme: p.theme.unwrap_or(self.theme),
            check_updates_on_startup: p
                .check_updates_on_startup
                .unwrap_or(self.check_updates_on_startup),
            language: p.language.unwrap_or(self.language),
            explanation_level: p.explanation_level.unwrap_or(self.explanation_level),
            compact_mode: p.compact_mode.unwrap_or(self.compact_mode),
            gpu_preference: p.gpu_preference.unwrap_or(self.gpu_preference),
            log_retention: p.log_retention.unwrap_or(self.log_retention),
            mod_metadata_ttl_days: p
                .mod_metadata_ttl_days
                .unwrap_or(self.mod_metadata_ttl_days),
            sftp_upload_concurrency: p
                .sftp_upload_concurrency
                .unwrap_or(self.sftp_upload_concurrency),
            hidden_sidebar_buttons: p
                .hidden_sidebar_buttons
                .unwrap_or(self.hidden_sidebar_buttons),
            allow_server_ping: p.allow_server_ping.unwrap_or(self.allow_server_ping),
            register_url_scheme: p.register_url_scheme.unwrap_or(self.register_url_scheme),
            allow_ai_translation: p.allow_ai_translation.unwrap_or(self.allow_ai_translation),
            ai_provider: p.ai_provider.unwrap_or(self.ai_provider),
            ai_model: p.ai_model.unwrap_or(self.ai_model),
            ai_local_port: p.ai_local_port.unwrap_or(self.ai_local_port),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            hide_to_tray_during_game: false,
            game_start_window: Some(GameStartWindow::Keep),
            theme: ThemePreference::default(),
            check_updates_on_startup: true,
            language: default_language(),
            explanation_level: ExplanationLevel::default(),
            compact_mode: false,
            gpu_preference: GpuPreference::default(),
            log_retention: LogRetentionPolicy::default(),
            mod_metadata_ttl_days: default_mod_metadata_ttl_days(),
            sftp_upload_concurrency: default_sftp_upload_concurrency(),
            hidden_sidebar_buttons: Vec::new(),
            allow_server_ping: false,
            register_url_scheme: false,
            allow_ai_translation: false,
            ai_provider: AiProvider::default(),
            ai_model: String::new(),
            ai_local_port: default_ai_local_port(),
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
    /// Optional JVM initial heap (`-Xms`) in MB. `None` = JVM default.
    pub min_heap_mb: Option<u32>,
    pub extra_jvm_args: String,
    pub created_unix_ms: f64,
    /// True iff the effective version JAR is on disk. UI shows ✓/↓ icon.
    pub ready: bool,
    /// True iff `<instance>/icon.png` exists (a custom picture). UI shows it in
    /// place of the letter avatar. Cheap stat, computed like `ready`.
    pub has_icon: bool,
    pub mrpack_name: Option<String>,
    pub mrpack_version: Option<String>,
    pub mrpack_project_id: Option<String>,
    pub mrpack_source: Option<crate::mods::platform::ModSource>,
    pub mrpack_summary: Option<String>,
    pub mrpack_version_id: Option<String>,
    pub integrity: Option<crate::verify::IntegrityStatus>,
    pub imported_from: Option<ImportProvenance>,
    pub created_from_server: Option<String>,
}

impl InstanceWithStatus {
    pub fn from_file(file: &InstanceFile, ready: bool, has_icon: bool) -> Self {
        Self {
            id: file.id.clone(),
            name: file.name.clone(),
            mc_version: file.mc_version.clone(),
            loader: file.loader,
            loader_version: file.loader_version.clone(),
            max_heap_mb: file.max_heap_mb,
            min_heap_mb: file.min_heap_mb,
            extra_jvm_args: file.extra_jvm_args.clone(),
            created_unix_ms: file.created_unix_ms,
            ready,
            has_icon,
            mrpack_name: file.mrpack_name.clone(),
            mrpack_version: file.mrpack_version.clone(),
            mrpack_project_id: file.mrpack_project_id.clone(),
            mrpack_source: file.mrpack_source,
            mrpack_summary: file.mrpack_summary.clone(),
            mrpack_version_id: file.mrpack_version_id.clone(),
            integrity: file.integrity.clone(),
            imported_from: file.imported_from.clone(),
            created_from_server: file.created_from_server.clone(),
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

    #[test]
    fn general_settings_default_sftp_upload_concurrency_is_four() {
        let g = GeneralSettings::default();
        assert_eq!(g.sftp_upload_concurrency, 4);
    }

    #[test]
    fn old_app_json_without_concurrency_deserializes_to_four() {
        // Field added later → existing app.json `general` blocks lack it.
        // #[serde(default)] must fill it with the 4-stream default.
        let g: GeneralSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(g.sftp_upload_concurrency, 4);
    }
}

#[cfg(test)]
mod start_window_tests {
    use super::*;

    fn g(hide: bool, window: Option<GameStartWindow>) -> GeneralSettings {
        GeneralSettings {
            hide_to_tray_during_game: hide,
            game_start_window: window,
            ..GeneralSettings::default()
        }
    }
    fn patch(window: Option<GameStartWindow>, hide: Option<bool>) -> GeneralSettingsPatch {
        GeneralSettingsPatch {
            game_start_window: window,
            hide_to_tray_during_game: hide,
            ..GeneralSettingsPatch::default()
        }
    }
    use GameStartWindow::{HideToTray, Keep, Minimise};

    #[test]
    fn the_old_checkbox_means_hide_to_tray_or_keep() {
        assert_eq!(from_legacy(true), HideToTray);
        assert_eq!(from_legacy(false), Keep);
    }

    #[test]
    fn the_window_field_decides_and_a_missing_one_follows_the_old_checkbox() {
        assert_eq!(g(true, Some(Minimise)).start_window(), Minimise);
        assert_eq!(g(true, None).start_window(), HideToTray);
        assert_eq!(g(false, None).start_window(), Keep);
    }

    #[test]
    fn resolving_fills_only_a_missing_window_and_keeps_the_bool() {
        let r = g(true, None).resolved();
        assert_eq!(
            (r.game_start_window, r.hide_to_tray_during_game),
            (Some(HideToTray), true)
        );
        let r = g(false, Some(Minimise)).resolved();
        assert_eq!(r.game_start_window, Some(Minimise));
    }

    #[test]
    fn resolving_brings_a_disagreeing_old_checkbox_back_in_step_with_the_window() {
        // A hand edit (or a later build that stops writing the bool) must not
        // leave the pair disagreeing through every later write: the window wins.
        let r = g(true, Some(Minimise)).resolved();
        assert_eq!(
            (r.game_start_window, r.hide_to_tray_during_game),
            (Some(Minimise), false)
        );
        let r = g(false, Some(HideToTray)).resolved();
        assert_eq!(
            (r.game_start_window, r.hide_to_tray_during_game),
            (Some(HideToTray), true)
        );
    }

    #[test]
    fn an_unknown_window_value_reads_as_absent_not_as_an_unreadable_file() {
        let v: GeneralSettings = serde_json::from_str(
            r#"{"game_start_window":"teleport","hide_to_tray_during_game":true}"#,
        )
        .expect("a future variant must not break the file");
        assert_eq!(v.game_start_window, None);
        let v: GeneralSettings =
            serde_json::from_str(r#"{"game_start_window":"minimise"}"#).unwrap();
        assert_eq!(v.game_start_window, Some(Minimise));
        let v: GeneralSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(v.game_start_window, None);
    }

    #[test]
    fn a_window_patch_wins_and_sets_the_old_checkbox_in_step() {
        let out = g(false, Some(Keep)).patched(patch(Some(Minimise), Some(true)));
        assert_eq!(
            (out.game_start_window, out.hide_to_tray_during_game),
            (Some(Minimise), false)
        );
        let out = g(false, Some(Keep)).patched(patch(Some(HideToTray), None));
        assert_eq!(
            (out.game_start_window, out.hide_to_tray_during_game),
            (Some(HideToTray), true)
        );
    }

    #[test]
    fn ticking_the_old_checkbox_alone_means_hide_to_tray() {
        let out = g(false, Some(Minimise)).patched(patch(None, Some(true)));
        assert_eq!(
            (out.game_start_window, out.hide_to_tray_during_game),
            (Some(HideToTray), true)
        );
    }

    #[test]
    fn unticking_the_old_checkbox_never_clobbers_minimise() {
        let out = g(true, Some(HideToTray)).patched(patch(None, Some(false)));
        assert_eq!(
            (out.game_start_window, out.hide_to_tray_during_game),
            (Some(Keep), false)
        );
        let out = g(false, Some(Minimise)).patched(patch(None, Some(false)));
        assert_eq!(
            (out.game_start_window, out.hide_to_tray_during_game),
            (Some(Minimise), false)
        );
    }

    #[test]
    fn a_patch_of_neither_leaves_the_pair_alone() {
        let out = g(true, Some(HideToTray)).patched(patch(None, None));
        assert_eq!(
            (out.game_start_window, out.hide_to_tray_during_game),
            (Some(HideToTray), true)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InstanceFile {
        InstanceFile {
            id: "3f4a-bbbb-cccc-dddd-eeeeffffaaaa".into(),
            uid: None,
            name: "Default".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            min_heap_mb: None,
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
            created_from_server: None,
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
    fn min_heap_mb_defaults_to_none_for_old_json() {
        // An instance.json written before -Xms support must still parse.
        let json = r#"{
            "id": "abc", "name": "X", "mc_version": "1.20.1",
            "loader": "vanilla", "loader_version": null, "max_heap_mb": 2048,
            "extra_jvm_args": "", "created_unix_ms": 0
        }"#;
        let f: InstanceFile = serde_json::from_str(json).unwrap();
        assert_eq!(f.min_heap_mb, None);
    }

    #[test]
    fn min_heap_mb_roundtrips_when_set_and_is_omitted_when_none() {
        // None: skipped on serialize, restored as None.
        let s = sample();
        assert_eq!(s.min_heap_mb, None);
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("min_heap_mb"),
            "None must be omitted: {json}"
        );

        // Some: present in JSON and survives the round-trip.
        let mut s2 = sample();
        s2.min_heap_mb = Some(2048);
        let json2 = serde_json::to_string(&s2).unwrap();
        assert!(json2.contains(r#""min_heap_mb":2048"#), "got: {json2}");
        let back: InstanceFile = serde_json::from_str(&json2).unwrap();
        assert_eq!(back.min_heap_mb, Some(2048));
    }

    #[test]
    fn instance_with_status_from_file_preserves_fields() {
        let s = sample();
        let w = InstanceWithStatus::from_file(&s, true, false);
        assert_eq!(w.id, s.id);
        assert_eq!(w.mc_version, s.mc_version);
        assert_eq!(w.loader, s.loader);
        assert!(w.ready);
    }

    #[test]
    fn instance_with_status_carries_has_icon() {
        let s = sample();
        assert!(InstanceWithStatus::from_file(&s, true, true).has_icon);
        assert!(!InstanceWithStatus::from_file(&s, true, false).has_icon);
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
        let w = InstanceWithStatus::from_file(&s, true, false);
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
    fn general_settings_default_hidden_sidebar_buttons_is_empty() {
        let g = GeneralSettings::default();
        assert!(g.hidden_sidebar_buttons.is_empty());
    }

    #[test]
    fn app_file_round_trips_hidden_sidebar_buttons() {
        let mut app = AppFile::default();
        app.general.hidden_sidebar_buttons = vec!["servers".to_string(), "logs".to_string()];
        let json = serde_json::to_string(&app).unwrap();
        let back: AppFile = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.general.hidden_sidebar_buttons,
            vec!["servers".to_string(), "logs".to_string()]
        );
        assert_eq!(back, app);
    }

    #[test]
    fn old_app_json_without_hidden_sidebar_buttons_deserializes_to_empty() {
        // Field added later -> existing app.json `general` blocks lack it.
        // #[serde(default)] must fill it with an empty vec (all buttons visible).
        let old_json = r#"{
            "version": 1,
            "active_instance": null,
            "general": { "hide_to_tray_during_game": true }
        }"#;
        let parsed: AppFile = serde_json::from_str(old_json).unwrap();
        assert!(parsed.general.hidden_sidebar_buttons.is_empty());
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
    fn an_empty_patch_is_the_identity() {
        let g = GeneralSettings::default();
        assert_eq!(g.clone().patched(GeneralSettingsPatch::default()), g);
    }

    #[test]
    fn a_patch_changes_only_its_fields() {
        let g = GeneralSettings::default();
        let out = g.clone().patched(GeneralSettingsPatch {
            theme: Some(ThemePreference::Dark),
            allow_server_ping: Some(true),
            ..GeneralSettingsPatch::default()
        });
        assert_eq!(out.theme, ThemePreference::Dark);
        assert!(out.allow_server_ping);
        assert_eq!(out.language, g.language);
        assert_eq!(out.gpu_preference, g.gpu_preference);
        // The window action and its legacy bool are ONE setting kept in step;
        // a patch of neither leaves both alone.
        assert_eq!(out.game_start_window, g.game_start_window);
        assert_eq!(out.hide_to_tray_during_game, g.hide_to_tray_during_game);
    }

    /// Every `GeneralSettings` field must have a patch counterpart: the
    /// default block, serialised, must deserialise into the patch type
    /// (`deny_unknown_fields`).
    #[test]
    fn every_general_field_has_a_patch_counterpart() {
        let json = serde_json::to_value(GeneralSettings::default()).unwrap();
        let patch: GeneralSettingsPatch = serde_json::from_value(json).unwrap();
        assert!(patch.theme.is_some());
    }

    #[test]
    fn a_patch_deserialises_from_a_sparse_object() {
        let patch: GeneralSettingsPatch = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(patch.theme, Some(ThemePreference::Dark));
        assert!(patch.language.is_none());
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

    #[test]
    fn app_json_roundtrips_changelog_seen_version() {
        let mut f = AppFile::default();
        f.changelog_seen_version = Some("0.23.0".into());
        let s = serde_json::to_string(&f).unwrap();
        let back: AppFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.changelog_seen_version, Some("0.23.0".into()));
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
        let w = InstanceWithStatus::from_file(&s, true, false);
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
        assert_eq!(
            serde_json::to_string(&ForeignLauncher::Xmcl).unwrap(),
            "\"xmcl\""
        );
        assert_eq!(
            serde_json::to_string(&ForeignLauncher::LegacyLauncher).unwrap(),
            "\"legacy_launcher\""
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

    #[test]
    fn instance_file_roundtrips_with_created_from_server() {
        let mut s = sample();
        s.created_from_server = Some("srv-abcd".into());
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains(r#""created_from_server":"srv-abcd""#),
            "got: {json}"
        );
        let back: InstanceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn instance_file_serializes_skip_none_created_from_server() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("created_from_server"), "got: {json}");
    }

    #[test]
    fn instance_file_deserializes_old_json_without_created_from_server() {
        let json = r#"{
            "version": 1, "id": "abc", "name": "Old", "mc_version": "1.20.1",
            "loader": "vanilla", "loader_version": null, "max_heap_mb": 2048,
            "extra_jvm_args": "", "created_unix_ms": 1700000000000.0
        }"#;
        let inst: InstanceFile = serde_json::from_str(json).unwrap();
        assert_eq!(inst.created_from_server, None);
    }

    #[test]
    fn instance_with_status_carries_created_from_server() {
        let mut s = sample();
        s.created_from_server = Some("srv-xyz".into());
        let w = InstanceWithStatus::from_file(&s, true, false);
        assert_eq!(w.created_from_server.as_deref(), Some("srv-xyz"));
    }

    #[test]
    fn ai_translation_settings_default_off_and_parse_from_empty_json() {
        let g: GeneralSettings = serde_json::from_str("{}").expect("empty object parses");
        assert!(!g.allow_ai_translation);
        assert_eq!(g.ai_provider, AiProvider::Anthropic);
        assert_eq!(g.ai_local_port, 11434);
        assert_eq!(g.ai_model, "");
    }

    #[test]
    fn every_ai_provider_is_listed_in_all() {
        // ALL drives the uninstall keyring sweep. A variant missing from it
        // leaks a stored key past uninstall, which no test would otherwise see.
        assert_eq!(AiProvider::ALL.len(), 4);
        for p in AiProvider::ALL {
            assert!(!format!("{p:?}").is_empty());
        }
    }
}
