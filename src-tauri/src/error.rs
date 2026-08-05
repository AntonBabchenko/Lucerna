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

/// Why a file the user picked is not a usable datapack. A typed reason rather
/// than a message, so the UI can localise it — the launcher ships in English
/// and Russian and a hand-written English sentence inside a `clean` error would
/// reach a Russian user untranslated.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DatapackRejection {
    /// The picked file is not a `.zip` (and is not a folder).
    NotAZip,
    /// Valid pack, wrong kind: it has a top-level `assets/` tree.
    IsAResourcePack,
    /// No `pack.mcmeta`, or no `data/` tree.
    NotAPack,
}

#[derive(Debug, Clone, ThisError, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Error {
    #[error("Network error fetching {url}: {details}")]
    Network { url: String, details: String },

    #[error("Refused a request to a host that is not on the allowlist: {url}")]
    HostNotAllowed { url: String },

    /// A user-consented outbound channel was used while its Settings
    /// permission is off. `channel` is the stable channel id (e.g.
    /// `"server_ping"`) so the UI can name the setting to turn on.
    #[error("Consented channel '{channel}' is turned off in settings")]
    ConsentedChannelDisabled { channel: String },

    #[error("Update check failed: {details}")]
    UpdateCheckFailed { details: String },

    #[error("Update verification failed: {details}")]
    UpdateVerificationFailed { details: String },

    #[error("Update install failed: {details}")]
    UpdateInstallFailed { details: String },

    #[error("Hash mismatch for {path}: expected {expected}, got {got}")]
    HashMismatch {
        path: String,
        expected: String,
        got: String,
    },

    #[error("Java spawn failed: {details}")]
    JavaSpawn { details: String },

    #[error("instance {instance_id} is already running")]
    AlreadyRunning { instance_id: String },

    #[error("Account not set — enter your name first")]
    AccountNotSet,

    #[error("an instance operation is already in progress or the game is running")]
    InstanceBusy,

    #[error("Invalid server address '{address}': {reason}")]
    QuickPlayAddressInvalid { address: String, reason: String },

    #[error("Microsoft sign-in cancelled")]
    AuthCancelled,

    #[error("Microsoft auth failed at {stage}: {details}")]
    AuthFailed { stage: String, details: String },

    #[error("This Microsoft account does not own Minecraft")]
    NoMinecraftProfile,

    #[error("Skin image is invalid: {details}")]
    CosmeticImageInvalid { details: String },

    #[error("Skin library: {details}")]
    SkinLibrary { details: String },

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

    #[error("Instance {id} was not imported from another launcher")]
    ImportNoProvenance { id: String },

    #[error("The original source folder no longer exists: {path}")]
    ImportSourceMissing { path: String },

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

    /// The proposed folder name reduced to nothing once normalised to ASCII.
    #[error("That name leaves no usable folder name")]
    InstanceDirNameEmpty,

    /// Another directory already occupies that name.
    #[error("A folder named '{name}' already exists")]
    InstanceDirNameTaken { name: String },

    /// A Windows reserved device name (CON, PRN, LPT1, …).
    #[error("'{name}' is a name Windows reserves for devices")]
    InstanceDirNameReserved { name: String },

    /// The directory could not be renamed because something holds it open.
    ///
    /// On Windows this arrives as `ERROR_ACCESS_DENIED`, and the everyday cause
    /// is an Explorer window sitting in the folder or one of its descendants —
    /// a handle on any descendant blocks renaming the ancestor. Carries the
    /// SOURCE directory name: the lock is on the folder being moved, not on the
    /// destination, and reporting the destination sent the first user who hit
    /// this looking at a path that did not exist yet.
    #[error("folder '{name}' is open in another program")]
    InstanceDirLocked { name: String },

    /// The path cannot be expressed in the system ANSI code page, so the JVM
    /// would receive it with `?` substituted and die on `InvalidPathException`
    /// before Minecraft starts.
    ///
    /// `data_root` separates the two remedies: renaming the instance folder
    /// fixes one, and only relocating the data root fixes the other. The UI
    /// shows a different message and a different action for each.
    #[error("This path cannot be read by Java on this system")]
    PathNotLaunchable { data_root: bool },

    #[error("Offline account name '{name}' is not valid for Minecraft (reason: {reason:?})")]
    OfflineNameInvalid {
        name: String,
        reason: crate::accounts::offline_name::OfflineNameRejection,
    },

    #[error("Network error talking to {url}: {details}")]
    ModsNetwork { url: String, details: String },

    #[error("CurseForge appears unreachable (network or region block) at {url}")]
    ModsPlatformUnreachable { url: String },

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

    #[error("Mod source {platform:?} has no per-mod browser — it is a modpack-only source")]
    ModsPlatformUnsupported {
        // Rust field named `platform` (not `source`) to avoid thiserror v2
        // treating it as Error::source(); serialized as `source` on the wire
        // so the TS bindings stay consistent with sibling Mods* variants.
        #[serde(rename = "source")]
        platform: crate::mods::platform::ModSource,
    },

    #[error("Unexpected response from {platform}: {details}")]
    ModsDecode {
        #[serde(rename = "source")]
        platform: String,
        details: String,
    },

    #[error("This source does not expose a changelog API")]
    ChangelogUnsupported,

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

    #[error(
        "Mod filename {filename} is unsafe (path separator or traversal); refusing to install"
    )]
    ModsUnsafeFilename { filename: String },

    #[error("Mod cache I/O error: {details}")]
    ModsCacheIo { details: String },

    #[error("Instance directory I/O error at {path}: {details}")]
    ModsInstancePath { path: String, details: String },

    #[error("Modpack archive is invalid: {details}")]
    ModpackInvalidArchive { details: String },

    #[error("Not a supported Lucerna import link: {reason}")]
    ImportUrlInvalid { reason: String },

    // Field is `platform`, not `source`: thiserror reserves a field literally
    // named `source` for the error-cause chain.
    #[error("Import by link is not supported for {platform} yet")]
    ImportUrlUnsupportedSource { platform: String },

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

    #[error("Modpack export failed: {details}")]
    ModpackExportFailed { details: String },

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

    #[error("Screenshot '{filename}' not found in instance {instance_id}")]
    ScreenshotNotFound {
        instance_id: String,
        filename: String,
    },

    #[error("Invalid screenshot filename '{name}': {reason}")]
    ScreenshotPathInvalid { name: String, reason: String },

    #[error("Backup '{filename}' not found for world '{world_folder}' in instance {instance_id}")]
    BackupNotFound {
        instance_id: String,
        world_folder: String,
        filename: String,
    },

    #[error("Backup '{filename}' is unreadable or corrupted: {details}")]
    BackupCorrupt { filename: String, details: String },

    #[error("The selected file or folder is not a Minecraft world (no level.dat found)")]
    WorldImportNotAWorld,

    #[error("Unsupported import source — choose a .zip file or a world folder")]
    WorldImportUnsupportedSource,

    #[error("World archive is invalid: {details}")]
    WorldImportInvalidArchive { details: String },

    #[error("World is too large to import: {size} > cap {cap}")]
    WorldImportTooLarge { size: f64, cap: f64 },

    #[error("Playtime I/O error: {details}")]
    PlaytimeIo { details: String },

    #[error("Tray I/O error: {details}")]
    TrayIo { details: String },

    #[error("Window I/O error: {details}")]
    WindowIo { details: String },

    #[error("mclo.gs upload failed: {details}")]
    McLogsUpload { details: String },

    #[error("Could not read {launcher} instance: {details}")]
    ImportInstanceUnreadable { launcher: String, details: String },

    #[error("Unsupported loader '{loader}' in imported instance")]
    ImportUnsupportedLoader { loader: String },

    #[error("Import source folder not recognized: {path}")]
    ImportSourceUnrecognized { path: String },

    #[error("servers.dat could not be parsed: {reason}")]
    ServersDatParse { reason: String },

    #[error("Invalid server name '{name}': {reason}")]
    SavedServerNameInvalid { name: String, reason: String },

    #[error("The saved server list changed — refresh and try again")]
    SavedServerListChanged,

    /// A curated `server.properties` field failed validation.
    #[error("invalid server property {key}={value}: {reason}")]
    ServerInvalidProperty {
        key: String,
        value: String,
        reason: String,
    },

    /// Attempt to build/start a server without an accepted EULA.
    #[error("Minecraft EULA not accepted for this server")]
    ServerEulaNotAccepted,

    /// Could not resolve the server jar source (no server download in the
    /// manifest, or a loader/version without a server build).
    #[error("server jar unavailable for {loader} {mc_version}: {reason}")]
    ServerJarUnavailable {
        loader: String,
        mc_version: String,
        reason: String,
    },

    /// installServer (Forge/NeoForge) failed or did not start.
    #[error("server installer failed for {loader}: {details}")]
    ServerInstallerFailed { loader: String, details: String },

    /// The server process failed to spawn.
    #[error("server process spawn failed: {details}")]
    ServerSpawnFailed { details: String },

    /// The server is already running.
    #[error("server already running: {id}")]
    ServerAlreadyRunning { id: String },

    /// The operation requires that no hosting upload is in flight, but one is.
    #[error("server upload in progress: {id}")]
    ServerUploadInProgress { id: String },

    /// The hosting upload was cancelled by the user.
    #[error("upload cancelled")]
    UploadCancelled,

    /// The operation requires a running server, but it is not running.
    #[error("server not running: {id}")]
    ServerNotRunning { id: String },

    /// The server name failed validation (empty / duplicate / too long).
    #[error("invalid server name: {reason}")]
    ServerNameInvalid { reason: String },

    /// The mod can't be removed/disabled — another mod that remains on the
    /// server depends on it (protects a working install from breakage).
    #[error("cannot remove {filename}: required by {required_by}")]
    ServerModRequiredByOther {
        filename: String,
        required_by: String,
    },

    /// A server mod/plugin file operation was rejected by validation (unsafe
    /// name, path escape, not a `.jar`, …) — a policy refusal, not a
    /// filesystem failure, so it must not masquerade as `Io`.
    #[error("server file '{filename}' rejected: {reason}")]
    ServerFileInvalid { filename: String, reason: String },

    /// The operation is not available for this server core (e.g. installing
    /// mods on a plugin core, or an unsupported core switch).
    #[error("not supported for this server core: {reason}")]
    ServerCoreUnsupported { reason: String },

    /// A lookup keyed on the installed mod/plugin list missed — the list
    /// changed since the UI fetched it. Refresh and retry.
    #[error("installed server content changed — refresh and try again")]
    ServerContentStale,

    /// A datapack toggle was asked for on a server whose world does not exist
    /// yet (no `level.dat`). Enabled/disabled state lives in `level.dat`, and
    /// Minecraft writes its own when it generates the world — a stub written
    /// here would not survive generation, and would hand the generator a file
    /// claiming a world exists with no version, seed or generator settings.
    #[error("this server's world has not been created yet — start the server once")]
    ServerWorldNotCreated,

    #[error("Import source is not a .zip file or a folder")]
    ServerImportUnsupportedSource,

    #[error("Server import archive is invalid: {details}")]
    ServerImportInvalidArchive { details: String },

    #[error("Server import is too large: {size} bytes (cap {cap})")]
    ServerImportTooLarge { size: f64, cap: f64 },

    #[error("This doesn't look like a Minecraft server")]
    ServerImportNotAServer,

    #[error("Server import session expired or was already used: {token}")]
    ServerImportStagingExpired { token: String },

    /// Server SFTP upload is not configured (no `UploadConfig`).
    #[error("server upload not configured")]
    UploadNotConfigured,

    /// Could not establish the SSH/SFTP connection to the user's server.
    #[error("SFTP connect failed: {details}")]
    SftpConnectFailed { details: String },

    /// Password authentication against the SFTP server failed.
    #[error("SFTP authentication failed: {details}")]
    SftpAuthFailed { details: String },

    /// The host-key fingerprint changed from the previously trusted one (TOFU).
    #[error("SFTP host key changed (possible MITM) — expected {expected}, got {got}")]
    SftpHostKeyMismatch { expected: String, got: String },

    /// A failure during SFTP file transfer (directory creation / write).
    #[error("SFTP transfer failed: {details}")]
    SftpTransferFailed { details: String },

    /// A data-root relocation was requested while a game or server is running.
    #[error("stop running games and servers before moving the data folder")]
    DataLocationBusy,

    /// The chosen target folder is invalid (relative / nested / non-empty / same).
    #[error("invalid data location: {reason}")]
    DataLocationInvalid { reason: String },

    /// The move failed partway; the original data is intact.
    #[error("data location migration failed: {reason}")]
    DataLocationMigrationFailed { reason: String },

    /// A data-creating or launching command was invoked while the configured
    /// data root is unavailable and the launcher is running from the temporary
    /// default fallback. Writing now would land in the wrong root.
    #[error(
        "your data folder is unavailable; reconnect it and restart before creating or launching"
    )]
    DataLocationUnavailable,

    /// A world's `level.dat` could not be read or rewritten. `reason` is a raw
    /// NBT/gzip library message — Opaque on the TS side.
    #[error("level.dat could not be parsed: {reason}")]
    LevelDatParse { reason: String },

    /// A file the user picked to install as a datapack failed content
    /// validation (wrong extension, or the zip classifies as something other
    /// than a datapack). `reason` is typed, not a message — see
    /// `DatapackRejection`'s doc comment for why.
    #[error("{filename} is not a usable datapack")]
    DatapackInvalid {
        filename: String,
        reason: DatapackRejection,
    },

    /// A datapack exceeded the buffering cap. Classification and hashing both
    /// hold the whole pack in memory, so an unbounded pack on the catalog's
    /// automated download path is a stall, not a slow click. Sizes are `f64`
    /// to match the rest of the datapack surface (specta has no u64).
    #[error("{filename} is {size_bytes} bytes, over the {limit_bytes} byte limit")]
    DatapackTooLarge {
        filename: String,
        size_bytes: f64,
        limit_bytes: f64,
    },

    /// Vanilla Tweaks publishes per Minecraft family, and the family derived
    /// from this version does not exist upstream — usually a Minecraft
    /// release VT has not caught up with. Deliberately not answered by
    /// falling back to an older family: that would promise a compatibility
    /// nobody checked.
    #[error("Vanilla Tweaks has no packs for Minecraft {mc_version}")]
    VanillaTweaksUnavailable { mc_version: String },

    /// Vanilla Tweaks refused to build the selection, or answered something
    /// we could not read. Carries the server's own message rather than one of
    /// ours: we do not know its failure modes, and inventing wording would
    /// hide theirs.
    #[error("Vanilla Tweaks could not build that selection: {message}")]
    VanillaTweaksBuildFailed { message: String },

    /// A translation the user typed failed Minecraft's `%s`/`%N$s` format
    /// grammar and was refused before it ever reached the override store.
    /// `reason` is typed, not a message — mirrors `DatapackInvalid`'s reason
    /// field, for the same "the UI localises it" argument documented on
    /// `l10n::validate::FormatError`.
    #[error("Translation for '{key}' is not valid: {reason:?}")]
    L10nTranslationInvalid {
        key: String,
        reason: crate::l10n::validate::FormatError,
    },

    /// `l10n_apply` could not determine the instance's resource-pack format:
    /// its client jar is missing, unreadable, or its `version.json` is a
    /// shape `l10n::pack_format` does not recognise. Most commonly because
    /// the instance has never been launched, so
    /// `versions/<mc_version>/<mc_version>.jar` does not exist yet.
    #[error("Could not determine the resource-pack format for Minecraft {mc_version}")]
    L10nFormatUnknown { mc_version: String },

    /// `l10n_apply` refused a Minecraft version below resource format 4
    /// (1.12.2 and older): FML/Forge on these versions loads a mod's own
    /// lang file AFTER the resource pack stack, so an applied override would
    /// silently have no effect (MinecraftForge #4907, closed stale, never
    /// fixed — see `l10n::pack_format`'s module doc).
    #[error("Minecraft {mc_version} is too old to apply a translation override pack")]
    L10nFormatTooOld { mc_version: String },

    /// The `namespace` parameter of `l10n_set_override` failed
    /// `l10n::scan::is_traversal_unsafe` — refused at the IPC boundary,
    /// before it can ever reach `NamespaceStore` or be persisted into the
    /// on-disk override store. Necessary because `store::store_path`'s
    /// percent-encoding only sanitises the FILE NAME the store lands at,
    /// never the `namespace` value persisted INSIDE the JSON body; without
    /// this check a value like `"../../evil"` would be silently written to
    /// disk and only dropped later, when a pack is actually built
    /// (`pack::build`'s own defence-in-depth guard, which just drops that
    /// one namespace rather than refusing the write in the first place).
    #[error("Invalid namespace '{namespace}': contains a path-traversal segment")]
    L10nNamespaceInvalid { namespace: String },

    /// Same defect as [`Error::L10nNamespaceInvalid`], for the `lang`
    /// parameter of `l10n_set_override`. Kept as a separate variant — rather
    /// than a shared `field` marker — so the UI copy can name "target
    /// language" without embedding a raw Rust field name; mirrors how
    /// `WorldPathInvalid`/`ScreenshotPathInvalid` stay two variants sharing
    /// one validator instead of a generic `PathInvalid { field, .. }`. Worth
    /// catching separately from a bad namespace: unlike a bad namespace
    /// (which `pack::build` merely drops), a bad `lang` refuses the WHOLE
    /// pack build for every namespace, because `lang` doubles as `code`,
    /// composed into every entry name in the archive.
    #[error("Invalid target language '{lang}': contains a path-traversal segment")]
    L10nLangInvalid { lang: String },

    /// The selected AI provider has no API key stored.
    #[error("No API key stored for AI provider {provider}")]
    L10nPrefillKeyMissing { provider: String },

    /// The provider answered, but not with a usable result. `status` is the
    /// HTTP status, or 0 when the failure was in the body rather than the
    /// transport. `details` is truncated — a provider error body can echo the
    /// API key back.
    #[error("AI provider {provider} failed ({status}): {details}")]
    L10nPrefillProvider {
        provider: String,
        status: u16,
        details: String,
    },

    /// A pre-fill run is already in flight for this instance.
    #[error("A translation pre-fill is already running for this instance")]
    L10nPrefillBusy,

    /// A share bundle failed whole-file validation — not a zip, no
    /// `lucerna-l10n.json`, a schema from a newer Lucerna, a filesystem-hostile
    /// language code, or over the size caps. `error` is typed rather than a
    /// message for the same reason `L10nTranslationInvalid`'s is: the UI
    /// localises it, and it drives different copy per case (a plain resource
    /// pack gets "install it as one instead", a future schema gets "update the
    /// launcher").
    #[error("Not a valid Lucerna translations file: {error:?}")]
    L10nShareBundleInvalid {
        error: crate::l10n::share::BundleError,
    },

    /// Share import (and, later, deleting stored translations) refused because
    /// an AI pre-fill run is writing the same global store files right now.
    /// `store::save` rewrites a whole file, so the two would be
    /// last-writer-wins and could silently destroy entries the user already
    /// paid a model for.
    #[error("A translation pre-fill is running; try again when it finishes")]
    L10nSharePrefillActive,

    /// The export destination's filename would be deleted by a later Apply:
    /// every apply sweeps `resourcepacks/` of files starting with
    /// `options_txt::PACK_PREFIX`. Refused outright rather than writing a
    /// bundle that destroys itself the next time the recipient applies.
    #[error("Export filename must not start with the reserved prefix")]
    L10nShareDestReserved,

    /// Nothing to export: no selected namespace holds an override for this
    /// language.
    #[error("Nothing to export for this selection")]
    L10nShareNothingToExport,
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

    /// Build a `ModsNetwork` error from a lower-level cause, carrying only the
    /// LEAF transport detail. When `cause` is already an
    /// `Error::Network { details, .. }` — the usual case, since it came straight
    /// from `network::request` — take that leaf `details` instead of
    /// re-Displaying the whole parent (which would embed the URL a second time
    /// and leak the English `#[error(..)]` text). Any other cause falls back to
    /// its `Display`.
    pub fn mods_network(url: impl Into<String>, cause: Error) -> Self {
        let details = match cause {
            Error::Network { details, .. } => details,
            other => other.to_string(),
        };
        Self::ModsNetwork {
            url: url.into(),
            details,
        }
    }

    pub fn io(path: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.into(),
            details: cause.to_string(),
        }
    }

    /// Validation refusal for a server mod/plugin file operation. Use this —
    /// not `Error::io` with a placeholder path — so the UI gets a typed,
    /// cleanly-rendered error instead of a fake filesystem failure.
    pub fn server_file_invalid(filename: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ServerFileInvalid {
            filename: filename.into(),
            reason: reason.into(),
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
    fn mods_network_carries_leaf_detail_from_nested_network() {
        // The mods layer wraps a lower-level network error. The constructor must
        // carry ONLY the leaf transport detail, not re-Display the parent (which
        // would re-embed the URL and leak the English `#[error(..)]` text).
        let url = "https://api.modrinth.com/v2/search?query=x";
        let leaf = "error sending request for url (https://api.modrinth.com/v2/search?query=x)";
        let inner = Error::network(url, leaf);
        let e = Error::mods_network(url, inner);
        match &e {
            Error::ModsNetwork { url: u, details } => {
                assert_eq!(u, url);
                assert_eq!(details, leaf);
                // The doubled wrapper is gone.
                let rendered = format!("{e}");
                assert!(
                    !rendered.contains("Network error fetching"),
                    "double-wrapped: {rendered}"
                );
            }
            other => panic!("expected ModsNetwork, got {other:?}"),
        }
    }

    #[test]
    fn mods_network_falls_back_to_display_for_non_network_cause() {
        let e = Error::mods_network(
            "https://x",
            Error::AlreadyRunning {
                instance_id: "abc".into(),
            },
        );
        match e {
            Error::ModsNetwork { details, .. } => {
                assert_eq!(details, "instance abc is already running");
            }
            other => panic!("expected ModsNetwork, got {other:?}"),
        }
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
    fn servers_dat_parse_serializes_with_tag() {
        let e = Error::ServersDatParse {
            reason: "bad tag".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"servers_dat_parse""#),
            "got: {json}"
        );
        assert!(json.contains(r#""reason":"bad tag""#), "got: {json}");
    }

    #[test]
    fn saved_server_name_invalid_serializes_with_tag() {
        let e = Error::SavedServerNameInvalid {
            name: "x".into(),
            reason: "empty name".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"saved_server_name_invalid""#),
            "got: {json}"
        );
    }

    #[test]
    fn saved_server_list_changed_serializes_as_unit() {
        let e = Error::SavedServerListChanged;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"saved_server_list_changed"}"#);
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
    fn dir_locked_serializes_with_tag_and_name() {
        let e = Error::InstanceDirLocked {
            name: "My-Pack".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"instance_dir_locked""#),
            "got: {json}"
        );
        assert!(json.contains(r#""name":"My-Pack""#), "got: {json}");
    }

    #[test]
    fn dir_name_taken_serializes_with_tag_and_name() {
        let e = Error::InstanceDirNameTaken {
            name: "Pack".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"instance_dir_name_taken""#),
            "got: {json}"
        );
        assert!(json.contains(r#""name":"Pack""#), "got: {json}");
    }

    #[test]
    fn path_not_launchable_carries_the_data_root_flag() {
        let json = serde_json::to_string(&Error::PathNotLaunchable { data_root: true }).unwrap();
        assert!(
            json.contains(r#""kind":"path_not_launchable""#),
            "got: {json}"
        );
        assert!(json.contains(r#""data_root":true"#), "got: {json}");
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
    fn offline_name_invalid_serializes_with_tag_and_reason() {
        let e = Error::OfflineNameInvalid {
            name: "Игрок".into(),
            reason: crate::accounts::offline_name::OfflineNameRejection::InvalidChars,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"offline_name_invalid""#),
            "got: {json}"
        );
        assert!(json.contains(r#""reason":"invalid_chars""#), "got: {json}");
        assert!(json.contains(r#""name":"Игрок""#), "got: {json}");
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
    fn mods_platform_unreachable_serializes_with_tag() {
        let e = Error::ModsPlatformUnreachable {
            url: "https://api.curseforge.com/v1/mods/search".into(),
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(
            j.contains(r#""kind":"mods_platform_unreachable""#),
            "got: {j}"
        );
        assert!(j.contains(r#""url":"https://api.curseforge.com/v1/mods/search""#));
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

    #[test]
    fn world_import_not_a_world_serializes_as_unit() {
        let e = Error::WorldImportNotAWorld;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"world_import_not_a_world"}"#);
    }

    #[test]
    fn world_import_unsupported_source_serializes_as_unit() {
        let e = Error::WorldImportUnsupportedSource;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"world_import_unsupported_source"}"#);
    }

    #[test]
    fn world_import_invalid_archive_carries_details() {
        let e = Error::WorldImportInvalidArchive {
            details: "unsafe path: ../x".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"world_import_invalid_archive""#),
            "got: {json}"
        );
        assert!(
            json.contains(r#""details":"unsafe path: ../x""#),
            "got: {json}"
        );
    }

    #[test]
    fn world_import_too_large_carries_size_and_cap() {
        let e = Error::WorldImportTooLarge {
            size: 3.0,
            cap: 2.0,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"world_import_too_large""#),
            "got: {json}"
        );
        assert!(json.contains(r#""size":3"#), "got: {json}");
        assert!(json.contains(r#""cap":2"#), "got: {json}");
    }

    #[test]
    fn data_location_busy_serializes_as_unit() {
        let e = Error::DataLocationBusy;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"data_location_busy"}"#);
    }

    #[test]
    fn data_location_invalid_carries_reason() {
        let e = Error::DataLocationInvalid {
            reason: "NotEmpty".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"data_location_invalid""#),
            "got: {json}"
        );
        assert!(json.contains(r#""reason":"NotEmpty""#), "got: {json}");
    }

    #[test]
    fn data_location_migration_failed_carries_reason() {
        let e = Error::DataLocationMigrationFailed {
            reason: "copy interrupted".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""kind":"data_location_migration_failed""#),
            "got: {json}"
        );
        assert!(
            json.contains(r#""reason":"copy interrupted""#),
            "got: {json}"
        );
    }

    #[test]
    fn data_location_unavailable_serializes_as_unit() {
        let e = Error::DataLocationUnavailable;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"kind":"data_location_unavailable"}"#);
    }

    #[test]
    fn level_dat_parse_serializes_with_its_kind_and_reason() {
        let e = Error::LevelDatParse {
            reason: "invalid tag id 99".into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "level_dat_parse");
        assert_eq!(v["reason"], "invalid tag id 99");
    }

    #[test]
    fn datapack_invalid_serializes_with_tag_filename_and_typed_reason() {
        let e = Error::DatapackInvalid {
            filename: "Faithful.zip".into(),
            reason: DatapackRejection::IsAResourcePack,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "datapack_invalid");
        assert_eq!(v["filename"], "Faithful.zip");
        // The reason is a typed enum tag, not a hand-written English sentence —
        // that's the whole point (see `DatapackRejection`'s doc comment).
        assert_eq!(v["reason"], "is_a_resource_pack");
    }

    #[test]
    fn l10n_translation_invalid_serializes_with_tag_key_and_nested_reason() {
        let e = Error::L10nTranslationInvalid {
            key: "item.create.wrench".into(),
            reason: crate::l10n::validate::FormatError::UnsupportedSpecifier { specifier: 'd' },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "l10n_translation_invalid");
        assert_eq!(v["key"], "item.create.wrench");
        // `FormatError` is itself internally tagged with `kind`; nested under
        // `reason` this cannot collide with the outer `Error`'s own `kind`.
        assert_eq!(v["reason"]["kind"], "unsupported_specifier");
        assert_eq!(v["reason"]["specifier"], "d");
    }

    #[test]
    fn l10n_format_unknown_carries_mc_version() {
        let e = Error::L10nFormatUnknown {
            mc_version: "1.20.1".into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "l10n_format_unknown");
        assert_eq!(v["mc_version"], "1.20.1");
    }

    #[test]
    fn l10n_format_too_old_carries_mc_version() {
        let e = Error::L10nFormatTooOld {
            mc_version: "1.12.2".into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "l10n_format_too_old");
        assert_eq!(v["mc_version"], "1.12.2");
    }

    #[test]
    fn l10n_namespace_invalid_serializes_with_tag_and_namespace() {
        let e = Error::L10nNamespaceInvalid {
            namespace: "../../evil".into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "l10n_namespace_invalid");
        assert_eq!(v["namespace"], "../../evil");
    }

    #[test]
    fn l10n_lang_invalid_serializes_with_tag_and_lang() {
        let e = Error::L10nLangInvalid {
            lang: "../../evil".into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "l10n_lang_invalid");
        assert_eq!(v["lang"], "../../evil");
    }
}
