//! Top-level error enum. Every fallible function in the launcher returns
//! `Result<T>` (alias for `std::result::Result<T, Error>`).
//!
//! `Error` derives `Serialize` + `specta::Type` so each variant crosses
//! the IPC boundary with its context intact — the UI gets typed errors,
//! not strings.

use serde::Serialize;
use specta::Type;
use thiserror::Error as ThisError;

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModsAuthKind {
    Missing,
    Invalid,
}

#[derive(Debug, Clone, ThisError, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Error {
    #[error("Network error fetching {url}: {details}")]
    Network { url: String, details: String },

    #[error("Refused a request to a host that is not on the allowlist: {url}")]
    HostNotAllowed { url: String },

    #[error("Hash mismatch for {path}: expected {expected}, got {got}")]
    HashMismatch {
        path: String,
        expected: String,
        got: String,
    },

    #[error("Java spawn failed: {details}")]
    JavaSpawn { details: String },

    #[error("Minecraft is already running")]
    AlreadyRunning,

    #[error("Account not set — enter your name first")]
    AccountNotSet,

    #[error("Microsoft sign-in cancelled")]
    AuthCancelled,

    #[error("Microsoft auth failed at {stage}: {details}")]
    AuthFailed { stage: String, details: String },

    #[error("This Microsoft account does not own Minecraft")]
    NoMinecraftProfile,

    #[error("Microsoft has not yet approved this launcher's app registration")]
    AuthPendingApproval,

    #[error("Version {id} not found in manifest")]
    UnknownVersion { id: String },

    #[error("{loader} does not support Minecraft {mc_version}")]
    LoaderUnavailable { loader: String, mc_version: String },

    #[error("Unsupported platform: {os}/{arch}")]
    UnsupportedPlatform { os: String, arch: String },

    #[error("IO error at {path}: {details}")]
    Io { path: String, details: String },

    #[error("Cannot delete the last instance — at least one must remain")]
    LastInstance,

    #[error("Active instance has no Minecraft version selected — pick one first")]
    NoVersionSelected,

    #[error("Instance {id} not found")]
    InstanceNotFound { id: String },

    #[error("Forge promotions info unavailable for {flavor}")]
    ForgePromotionsUnavailable { flavor: String },

    #[error("Forge maven-metadata.xml could not be parsed: {details}")]
    ForgeMavenMetadataParseFailed { details: String },

    #[error("No Forge build exists for Minecraft {mc} (tried {fv})")]
    ForgeNoBuildFor { mc: String, fv: String },

    #[error("Forge installer for {mc}-{fv} is corrupted: {details}")]
    ForgeInstallerCorrupted {
        mc: String,
        fv: String,
        details: String,
    },

    #[error("This Forge version uses an unsupported processor: {coord}")]
    ForgeUnsupportedProcessor { coord: String },

    #[error("Forge installation failed during {processor}: {details}")]
    ForgePatcherFailed { processor: String, details: String },

    #[error("Mappings for Minecraft {mc} are unavailable")]
    ForgeMappingsMissing { mc: String },

    #[error("Instance name cannot be empty")]
    InstanceNameEmpty,

    #[error("Instance name is too long: {actual} characters (max {max})")]
    InstanceNameTooLong { max: u32, actual: u32 },

    #[error("Network error talking to {url}: {details}")]
    ModsNetwork { url: String, details: String },

    #[error("Mod platform auth: {kind:?}")]
    ModsPlatformAuth {
        // Rust field stays `kind` per spec; serialized as `kind_detail`
        // to avoid colliding with the enum's serde `tag = "kind"`.
        #[serde(rename = "kind_detail")]
        kind: ModsAuthKind,
    },

    #[error("Mod {project_id} on {platform}: distribution disabled by author")]
    ModsDistributionDisabled {
        // Rust field renamed from `source` to `platform` because thiserror v2
        // treats fields named `source` as `Error::source()`; on the wire and
        // in TS bindings we keep `source` via serde rename.
        #[serde(rename = "source")]
        platform: String,
        project_id: String,
    },

    #[error("Mod project not found on {platform}")]
    ModsNotFound {
        #[serde(rename = "source")]
        platform: String,
    },

    #[error("Unexpected response from {platform}: {details}")]
    ModsDecode {
        #[serde(rename = "source")]
        platform: String,
        details: String,
    },

    #[error("Mod file has no SHA-1 published; refusing to install")]
    ModsSha1Unavailable,

    #[error("SHA-1 mismatch: expected {expected}, got {got}")]
    ModsSha1Mismatch { expected: String, got: String },

    #[error("Dependency {project_ref} could not be resolved for this MC + loader")]
    ModsDependencyUnresolvable { project_ref: String },

    #[error("Cannot place {filename}: a different file with this name already exists")]
    ModsFilenameConflict {
        filename: String,
        existing_sha: String,
        incoming_sha: String,
    },

    #[error("Mod cache I/O error: {details}")]
    ModsCacheIo { details: String },

    #[error("Instance directory I/O error at {path}: {details}")]
    ModsInstancePath { path: String, details: String },

    #[error("Modpack archive is invalid: {details}")]
    ModpackInvalidArchive { details: String },

    #[error("Modpack format unknown — no modrinth.index.json or manifest.json found")]
    ModpackFormatUnknown,

    #[error("Modpack {format} manifest is invalid: {details}")]
    ModpackManifestInvalid { format: String, details: String },

    #[error("Modpack {format} manifest version {version} is not supported")]
    ModpackUnsupportedManifestVersion { format: String, version: u32 },

    #[error("Modpack {format} declares unsupported loader: {loader_id}")]
    ModpackUnsupportedLoader { format: String, loader_id: String },

    #[error("Modpack file {file_path} references host {host} which is not on the allowlist")]
    ModpackDownloadHostNotAllowed { host: String, file_path: String },

    #[error("Modpack file {mod_name} has no SHA-1 in the manifest")]
    ModpackSha1Unavailable { mod_name: String },

    #[error("Mod '{mod_name}' cannot be distributed by third parties — download manually from {project_url}")]
    ModpackModDistributionDisabled {
        mod_name: String,
        project_url: String,
    },

    #[error("Modpack overrides entry escapes the instance directory: {entry}")]
    ModpackOverridesPathEscape { entry: String },

    #[error("Modpack overrides entry {entry} is too large: {size} > cap {cap}")]
    ModpackOverridesTooLarge { entry: String, size: f64, cap: f64 },

    #[error("Modpack picker had no files selected")]
    ModpackNoFilesSelected,

    #[error("Modpack instance creation failed: {details}")]
    ModpackInstanceCreationFailed { details: String },

    // Display intentionally omits `failed.len()`; the FE handler renders the
    // count from `.failed.length` (thiserror 2.0 disallows function-call
    // expressions in `#[error("...")]` format strings).
    #[error("Modpack import partially failed for instance {instance_id}")]
    ModpackPartialFailure {
        instance_id: String,
        failed: Vec<(String, String)>,
    },

    #[error("Mod '{mod_name}' was bundled inside the .mrpack archive and cannot be restored automatically — re-import the pack to recover it")]
    ModpackBundledNoUrl { mod_name: String },

    #[error("The CurseForge modpack '{pack_name}' cannot be downloaded by third-party launchers — its author disabled distribution. Open it on CurseForge and install the .zip manually.")]
    ModpackCfDistributionDisabled { pack_name: String },

    #[error("World '{folder_name}' not found in instance {instance_id}")]
    WorldNotFound {
        instance_id: String,
        folder_name: String,
    },

    #[error("World '{folder_name}' is currently in use — quit Minecraft and try again")]
    WorldInUse { folder_name: String },

    #[error("Invalid world or backup name '{name}': {reason}")]
    WorldPathInvalid { name: String, reason: String },

    #[error("Could not resolve a free name for '{folder_name}' after trying 99 suffixes")]
    WorldNameUnresolvable { folder_name: String },

    #[error("Backup '{filename}' not found for world '{world_folder}' in instance {instance_id}")]
    BackupNotFound {
        instance_id: String,
        world_folder: String,
        filename: String,
    },

    #[error("Backup '{filename}' is unreadable or corrupted: {details}")]
    BackupCorrupt { filename: String, details: String },

    #[error("Playtime I/O error: {details}")]
    PlaytimeIo { details: String },

    #[error("Tray I/O error: {details}")]
    TrayIo { details: String },

    #[error("mclo.gs upload failed: {details}")]
    McLogsUpload { details: String },
}

pub type Result<T> = std::result::Result<T, Error>;

// Convenience constructors for the most common conversions. Inline `?`
// at the call site is otherwise tedious because the variants want
// context strings.

impl Error {
    pub fn network(url: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        Self::Network {
            url: url.into(),
            details: cause.to_string(),
        }
    }

    pub fn io(path: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.into(),
            details: cause.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_constructor_includes_context() {
        let e = Error::network("https://example.com/x", "connection refused");
        let msg = format!("{e}");
        assert!(msg.contains("https://example.com/x"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn hash_mismatch_serializes_with_tag() {
        let e = Error::HashMismatch {
            path: "/tmp/x".into(),
            expected: "aaa".into(),
            got: "bbb".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        // tag: "kind" + snake_case rename → "hash_mismatch"
        assert!(json.contains(r#""kind":"hash_mismatch""#), "got: {json}");
        assert!(json.contains(r#""expected":"aaa""#));
    }

    #[test]
    fn loader_unavailable_serializes_with_tag() {
        let e = Error::LoaderUnavailable {
            loader: "fabric".into(),
            mc_version: "1.6.4".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"loader_unavailable""#),
            "got: {json}"
        );
        assert!(json.contains(r#""loader":"fabric""#), "got: {json}");
        assert!(json.contains(r#""mc_version":"1.6.4""#), "got: {json}");
    }

    #[test]
    fn last_instance_serializes_with_tag() {
        let e = Error::LastInstance;
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"last_instance""#), "got: {json}");
    }

    #[test]
    fn no_version_selected_serializes_with_tag() {
        let e = Error::NoVersionSelected;
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"no_version_selected""#),
            "got: {json}"
        );
    }

    #[test]
    fn instance_not_found_carries_id() {
        let e = Error::InstanceNotFound {
            id: "3f4a-bbbb".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"instance_not_found""#),
            "got: {json}"
        );
        assert!(json.contains(r#""id":"3f4a-bbbb""#), "got: {json}");
    }

    #[test]
    fn forge_promotions_unavailable_serializes_with_tag() {
        let e = Error::ForgePromotionsUnavailable {
            flavor: "forge".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"forge_promotions_unavailable""#),
            "got: {json}"
        );
        assert!(json.contains(r#""flavor":"forge""#), "got: {json}");
    }

    #[test]
    fn forge_maven_metadata_parse_failed_carries_details() {
        let e = Error::ForgeMavenMetadataParseFailed {
            details: "unexpected EOF".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"forge_maven_metadata_parse_failed""#),
            "got: {json}"
        );
        assert!(
            json.contains(r#""details":"unexpected EOF""#),
            "got: {json}"
        );
    }

    #[test]
    fn forge_installer_corrupted_carries_context() {
        let e = Error::ForgeInstallerCorrupted {
            mc: "1.20.4".into(),
            fv: "49.0.49".into(),
            details: "missing install_profile.json".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"forge_installer_corrupted""#),
            "got: {json}"
        );
        assert!(json.contains(r#""mc":"1.20.4""#), "got: {json}");
        assert!(json.contains(r#""fv":"49.0.49""#), "got: {json}");
    }

    #[test]
    fn forge_unsupported_processor_carries_coord() {
        let e = Error::ForgeUnsupportedProcessor {
            coord: "net.example:tool:1.0".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"forge_unsupported_processor""#),
            "got: {json}"
        );
        assert!(
            json.contains(r#""coord":"net.example:tool:1.0""#),
            "got: {json}"
        );
    }

    #[test]
    fn forge_patcher_failed_carries_processor_name() {
        let e = Error::ForgePatcherFailed {
            processor: "BinaryPatcher".into(),
            details: "lzma decode error".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"forge_patcher_failed""#),
            "got: {json}"
        );
        assert!(
            json.contains(r#""processor":"BinaryPatcher""#),
            "got: {json}"
        );
    }

    #[test]
    fn forge_mappings_missing_carries_mc() {
        let e = Error::ForgeMappingsMissing {
            mc: "1.20.4".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"forge_mappings_missing""#),
            "got: {json}"
        );
        assert!(json.contains(r#""mc":"1.20.4""#), "got: {json}");
    }

    #[test]
    fn instance_name_empty_serializes_with_tag() {
        let e = Error::InstanceNameEmpty;
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"instance_name_empty""#),
            "got: {json}"
        );
    }

    #[test]
    fn instance_name_too_long_carries_max_and_actual() {
        let e = Error::InstanceNameTooLong {
            max: 32,
            actual: 50,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"instance_name_too_long""#),
            "got: {json}"
        );
        assert!(json.contains(r#""max":32"#), "got: {json}");
        assert!(json.contains(r#""actual":50"#), "got: {json}");
    }

    #[test]
    fn mods_network_serializes_with_tag() {
        let e = Error::ModsNetwork {
            url: "https://api.modrinth.com/v2/search".into(),
            details: "timeout".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains(r#""kind":"mods_network""#), "got: {j}");
        assert!(j.contains(r#""url":"https://api.modrinth.com/v2/search""#));
    }

    #[test]
    fn mods_platform_auth_carries_kind() {
        let e = Error::ModsPlatformAuth {
            kind: ModsAuthKind::Missing,
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains(r#""kind":"mods_platform_auth""#), "got: {j}");
        assert!(j.contains(r#""kind_detail":"missing""#), "got: {j}");
    }

    #[test]
    fn mods_sha1_mismatch_carries_expected_and_got() {
        let e = Error::ModsSha1Mismatch {
            expected: "aaa".into(),
            got: "bbb".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains(r#""kind":"mods_sha1_mismatch""#));
        assert!(j.contains(r#""expected":"aaa""#));
        assert!(j.contains(r#""got":"bbb""#));
    }

    #[test]
    fn mods_filename_conflict_carries_both_hashes() {
        let e = Error::ModsFilenameConflict {
            filename: "jei.jar".into(),
            existing_sha: "111".into(),
            incoming_sha: "222".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains(r#""kind":"mods_filename_conflict""#));
        assert!(j.contains(r#""filename":"jei.jar""#));
    }

    #[test]
    fn modpack_invalid_archive_serializes() {
        let e = Error::ModpackInvalidArchive {
            details: "not zip".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"modpack_invalid_archive""#),
            "got: {json}"
        );
        assert!(json.contains(r#""details":"not zip""#), "got: {json}");
    }

    #[test]
    fn modpack_format_unknown_serializes_as_unit() {
        let e = Error::ModpackFormatUnknown;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"modpack_format_unknown"}"#);
    }

    #[test]
    fn modpack_partial_failure_serializes_with_list() {
        let e = Error::ModpackPartialFailure {
            instance_id: "abc".into(),
            failed: vec![("mods/foo.jar".into(), "404 from cdn".into())],
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"modpack_partial_failure""#),
            "got: {json}"
        );
        assert!(json.contains(r#""instance_id":"abc""#), "got: {json}");
        assert!(
            json.contains(r#""failed":[["mods/foo.jar","404 from cdn"]]"#),
            "got: {json}"
        );
    }

    #[test]
    fn host_not_allowed_serializes_with_tag_and_url() {
        let e = Error::HostNotAllowed {
            url: "http://evil.example/x".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"host_not_allowed""#), "got: {json}");
        assert!(
            json.contains(r#""url":"http://evil.example/x""#),
            "got: {json}"
        );
    }
}
