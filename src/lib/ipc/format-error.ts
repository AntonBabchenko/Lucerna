import { get } from 'svelte/store';
import { offlineNameRejectionKey } from '$lib/accounts/offline-name';
import { t } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import { displayLoader } from '$lib/instances/loader-display';
import type {
  DatapackRejection,
  FormatError,
  Error as IpcError,
  LoaderKind,
} from '$lib/ipc/bindings';

// Detail-bearing errors are truncated to this many code points in the UI; the
// full text lives in the launcher log. Slicing is by code point (spread), not
// UTF-16 unit, so a surrogate pair at the boundary is never split.
const DETAIL_TRUNCATE_CODE_POINTS = 120;

// Stable channel ids from `network::consent::ConsentedChannel::id()` mapped to
// the display name of the setting that turns them on. An unknown id (a newer
// backend against an older UI) falls back to a generic phrase rather than
// printing a raw identifier at the user.
const CONSENTED_CHANNEL_LABELS: Record<string, TranslationKey> = {
  server_ping: 'settings.general.serverPing.label',
};

/**
 * Render an Opaque error's `headline` plus its raw `detail`:
 *   - no detail            → `headline`
 *   - detail ≤ 120 cp      → `headline: detail`
 *   - detail >  120 cp     → `headline: <first 120>… (open Logs for full text)`
 *
 * Single source of truth for the "human sentence + truncated detail + point at
 * Logs" convention. Every Opaque variant in formatError routes through here so
 * the formatting is identical everywhere (generalises the old inline `io` case).
 */
export function withDetailTail(headline: string, raw: string | null | undefined): string {
  if (!raw) return headline;
  const codePoints = [...raw];
  if (codePoints.length <= DETAIL_TRUNCATE_CODE_POINTS) {
    return `${headline}: ${raw}`;
  }
  const hint = get(t)('errors.ioTruncatedHint');
  return `${headline}: ${codePoints.slice(0, DETAIL_TRUNCATE_CODE_POINTS).join('')}… (${hint})`;
}

export type ErrorClass = 'clean' | 'transport' | 'opaque';

/**
 * The display-policy registry. Every Error variant is assigned exactly one
 * class; the `Record<IpcError['kind'], …>` type makes a missing assignment a
 * compile error, so a new variant in bindings.ts cannot land unclassified. The
 * vitest sweep enforces the per-class invariants at runtime (transport hides
 * url/detail; opaque truncates + points at Logs). Together they keep every
 * error rendered consistently.
 *
 *   clean     — message fully built from structured fields (the default).
 *   transport — network/availability; clean actionable, no raw detail.
 *   opaque    — parse/io/spawn; headline + truncated detail + "open Logs".
 */
export const ERROR_CLASS: Record<IpcError['kind'], ErrorClass> = {
  // Transport — clean actionable, detail to log only.
  network: 'transport',
  mods_network: 'transport',
  mods_platform_unreachable: 'transport',
  // Opaque — headline + truncated detail + "open Logs".
  io: 'opaque',
  java_spawn: 'opaque',
  auth_failed: 'opaque',
  forge_maven_metadata_parse_failed: 'opaque',
  forge_installer_corrupted: 'opaque',
  forge_patcher_failed: 'opaque',
  mods_decode: 'opaque',
  mods_cache_io: 'opaque',
  mods_instance_path: 'opaque',
  modpack_invalid_archive: 'opaque',
  modpack_manifest_invalid: 'opaque',
  modpack_instance_creation_failed: 'opaque',
  modpack_export_failed: 'opaque',
  backup_corrupt: 'opaque',
  world_import_invalid_archive: 'opaque',
  playtime_io: 'opaque',
  tray_io: 'opaque',
  window_io: 'opaque',
  update_check_failed: 'opaque',
  update_verification_failed: 'opaque',
  update_install_failed: 'opaque',
  import_instance_unreadable: 'opaque',
  server_spawn_failed: 'opaque',
  server_installer_failed: 'opaque',
  sftp_connect_failed: 'opaque',
  sftp_transfer_failed: 'opaque',
  mc_logs_upload: 'opaque',
  server_import_invalid_archive: 'opaque',
  servers_dat_parse: 'opaque',
  level_dat_parse: 'opaque',
  cosmetic_image_invalid: 'opaque',
  skin_library: 'opaque',
  // Clean — everything else (self-contained from structured fields).
  host_not_allowed: 'clean',
  consented_channel_disabled: 'clean',
  hash_mismatch: 'clean',
  already_running: 'clean',
  account_not_set: 'clean',
  instance_busy: 'clean',
  auth_cancelled: 'clean',
  no_minecraft_profile: 'clean',
  auth_pending_approval: 'clean',
  unknown_version: 'clean',
  unsupported_platform: 'clean',
  loader_unavailable: 'clean',
  last_instance: 'clean',
  no_version_selected: 'clean',
  instance_not_found: 'clean',
  forge_promotions_unavailable: 'clean',
  forge_no_build_for: 'clean',
  forge_unsupported_processor: 'clean',
  forge_mappings_missing: 'clean',
  instance_name_empty: 'clean',
  instance_name_too_long: 'clean',
  mods_platform_auth: 'clean',
  mods_distribution_disabled: 'clean',
  mods_not_found: 'clean',
  mods_platform_unsupported: 'clean',
  changelog_unsupported: 'clean',
  mods_sha1_unavailable: 'clean',
  mods_sha1_mismatch: 'clean',
  mods_dependency_unresolvable: 'clean',
  mods_filename_conflict: 'clean',
  mods_unsafe_filename: 'clean',
  modpack_format_unknown: 'clean',
  // Import-by-link: both carry a self-contained, user-facing reason built in
  // Rust from the link itself — nothing to truncate or hide.
  import_url_invalid: 'clean',
  import_url_unsupported_source: 'clean',
  modpack_unsupported_manifest_version: 'clean',
  modpack_unsupported_loader: 'clean',
  modpack_download_host_not_allowed: 'clean',
  modpack_sha1_unavailable: 'clean',
  modpack_mod_distribution_disabled: 'clean',
  modpack_overrides_path_escape: 'clean',
  modpack_overrides_too_large: 'clean',
  modpack_no_files_selected: 'clean',
  modpack_partial_failure: 'clean',
  modpack_bundled_no_url: 'clean',
  modpack_cf_distribution_disabled: 'clean',
  world_not_found: 'clean',
  world_in_use: 'clean',
  world_path_invalid: 'clean',
  world_name_unresolvable: 'clean',
  screenshot_not_found: 'clean',
  screenshot_path_invalid: 'clean',
  backup_not_found: 'clean',
  world_import_not_a_world: 'clean',
  world_import_unsupported_source: 'clean',
  world_import_too_large: 'clean',
  quick_play_address_invalid: 'clean',
  import_unsupported_loader: 'clean',
  import_source_unrecognized: 'clean',
  import_no_provenance: 'clean',
  import_source_missing: 'clean',
  saved_server_name_invalid: 'clean',
  offline_name_invalid: 'clean',
  saved_server_list_changed: 'clean',
  server_invalid_property: 'clean',
  server_eula_not_accepted: 'clean',
  server_jar_unavailable: 'clean',
  server_already_running: 'clean',
  server_upload_in_progress: 'clean',
  upload_cancelled: 'clean',
  server_not_running: 'clean',
  server_mod_required_by_other: 'clean',
  server_file_invalid: 'clean',
  server_core_unsupported: 'clean',
  server_content_stale: 'clean',
  server_name_invalid: 'clean',
  upload_not_configured: 'clean',
  sftp_auth_failed: 'clean',
  sftp_host_key_mismatch: 'clean',
  server_import_unsupported_source: 'clean',
  server_import_too_large: 'clean',
  server_import_not_a_server: 'clean',
  server_import_staging_expired: 'clean',
  data_location_busy: 'clean',
  data_location_invalid: 'clean',
  data_location_migration_failed: 'opaque',
  data_location_unavailable: 'clean',
  // Built entirely from structured fields (filename + typed reason) — see
  // `datapackRejectionKey` below for why the reason is a typed lookup rather
  // than a raw string.
  datapack_invalid: 'clean',
  // In-game mod localization — the override editor. All three are built
  // entirely from structured fields; nothing to truncate or hide.
  l10n_translation_invalid: 'clean',
  l10n_format_unknown: 'clean',
  l10n_format_too_old: 'clean',
};

/**
 * Map a backend `DataLocationInvalid` reason token (a stable snake_case key —
 * `not_absolute` | `nested` | `same` | `not_empty`) to a translated,
 * human-readable clause. Never echoes the raw token to the user; an
 * unrecognized token falls back to a generic "not a valid location" clause.
 */
function dataLocationInvalidReason(reason: string): string {
  const translate = get(t);
  switch (reason) {
    case 'not_absolute':
      return translate('errors.dataLocationInvalidReason.notAbsolute');
    case 'nested':
      return translate('errors.dataLocationInvalidReason.nested');
    case 'same':
      return translate('errors.dataLocationInvalidReason.same');
    case 'not_empty':
      return translate('errors.dataLocationInvalidReason.notEmpty');
    case 'not_a_data_root':
      return translate('errors.dataLocationInvalidReason.notADataRoot');
    case 'not_writable':
      return translate('errors.dataLocationInvalidReason.notWritable');
    default:
      return translate('errors.dataLocationInvalidReason.unknown');
  }
}

/**
 * Map a backend `DatapackRejection` (a typed enum, not a message — see the
 * variant's doc comment in `error.rs`) to its translation key. The switch has
 * no `default` fallthrough: every arm returns directly, so a new
 * `DatapackRejection` variant that isn't handled here falls through to the
 * `_exhaustive: never` assignment below and fails to compile, rather than
 * silently rendering nothing for the new reason.
 */
function datapackRejectionKey(reason: DatapackRejection): TranslationKey {
  switch (reason) {
    case 'not_a_zip':
      return 'errors.datapackInvalidReason.notAZip';
    case 'is_a_resource_pack':
      return 'errors.datapackInvalidReason.isAResourcePack';
    case 'not_a_pack':
      return 'errors.datapackInvalidReason.notAPack';
    default: {
      const _exhaustive: never = reason;
      return _exhaustive;
    }
  }
}

/**
 * Render a backend `FormatError` (Minecraft's `%s`/`%N$s` grammar rejection —
 * see the variant's doc comment in `l10n/validate.rs`) as a translated
 * clause. Unlike `datapackRejectionKey`, this returns the rendered STRING
 * directly rather than a `TranslationKey`: each variant carries different
 * fields to interpolate (the offending specifier, or the index/available
 * pair), so there is no single flat `values` shape a caller could pass to
 * `translate()` uniformly. The switch has no `default` fallthrough, so a new
 * `FormatError` variant that isn't handled here fails to compile rather than
 * silently rendering nothing.
 */
function formatErrorReason(reason: FormatError): string {
  const translate = get(t);
  switch (reason.kind) {
    case 'unsupported_specifier':
      return translate('errors.l10nTranslationInvalidReason.unsupportedSpecifier', {
        specifier: reason.specifier,
      });
    case 'dangling_percent':
      return translate('errors.l10nTranslationInvalidReason.danglingPercent');
    case 'index_out_of_range':
      return translate('errors.l10nTranslationInvalidReason.indexOutOfRange', {
        index: reason.index,
        available: reason.available,
      });
    default: {
      const _exhaustive: never = reason;
      return _exhaustive;
    }
  }
}

/**
 * Render a typed IPC Error as a human-readable single-line string.
 *
 * Single source of truth for error text shown anywhere in the UI.
 * Context-specific wrappers (the modal's ipcErrorMessage, the picker's
 * formatLoaderError) handle a couple of cases with shorter context-aware
 * wording, then delegate here for everything else — so no JSON.stringify
 * leak for unhandled variants.
 *
 * The switch is exhaustive over every variant of the Error union as of
 * the bindings.ts at this commit. Adding a new variant to error.rs
 * without extending this function would surface as a TypeScript error
 * at the `_exhaustive: never` line, not as a runtime JSON leak.
 */
export function formatError(e: IpcError): string {
  const translate = get(t);
  switch (e.kind) {
    case 'network':
      return translate('errors.network');
    case 'host_not_allowed':
      return translate('errors.hostNotAllowed', { url: e.url });
    case 'consented_channel_disabled':
      return translate('errors.consentedChannelDisabled', {
        channel: translate(CONSENTED_CHANNEL_LABELS[e.channel] ?? 'errors.consentedChannelGeneric'),
      });
    case 'hash_mismatch':
      return translate('errors.hashMismatch', { path: e.path });
    case 'java_spawn':
      return withDetailTail(translate('errors.javaSpawn'), e.details);
    case 'already_running':
      return translate('errors.alreadyRunning');
    case 'account_not_set':
      return translate('errors.accountNotSet');
    case 'offline_name_invalid':
      return translate('errors.offlineNameInvalid', {
        name: e.name,
        reason: translate(offlineNameRejectionKey(e.reason)),
      });
    case 'instance_busy':
      return translate('errors.instanceBusy');
    case 'auth_cancelled':
      return translate('errors.authCancelled');
    case 'auth_failed':
      return withDetailTail(translate('errors.authFailed', { stage: e.stage }), e.details);
    case 'no_minecraft_profile':
      return translate('errors.noMinecraftProfile');
    case 'auth_pending_approval':
      return translate('errors.authPendingApproval');
    case 'cosmetic_image_invalid':
      return withDetailTail(translate('errors.cosmeticImageInvalid'), e.details);
    case 'skin_library':
      return withDetailTail(translate('errors.skinLibrary'), e.details);
    case 'unknown_version':
      return translate('errors.unknownVersion', { id: e.id });
    case 'unsupported_platform':
      return translate('errors.unsupportedPlatform', { os: e.os, arch: e.arch });
    case 'loader_unavailable':
      return translate('errors.loaderUnavailable', {
        loader: displayLoader(e.loader as LoaderKind),
        mcVersion: e.mc_version,
      });
    case 'last_instance':
      return translate('errors.lastInstance');
    case 'no_version_selected':
      return translate('errors.noVersionSelected');
    case 'instance_not_found':
      return translate('errors.instanceNotFound', { id: e.id });
    case 'io':
      return withDetailTail(translate('errors.io', { path: e.path }), e.details);
    case 'forge_promotions_unavailable':
      return translate('errors.forgePromotionsUnavailable', { flavor: e.flavor });
    case 'forge_maven_metadata_parse_failed':
      return withDetailTail(translate('errors.forgeMavenMetadataParseFailed'), e.details);
    case 'forge_no_build_for':
      return translate('errors.forgeNoBuildFor', { mc: e.mc });
    case 'forge_installer_corrupted':
      return withDetailTail(
        translate('errors.forgeInstallerCorrupted', { mc: e.mc, fv: e.fv }),
        e.details,
      );
    case 'forge_unsupported_processor':
      return translate('errors.forgeUnsupportedProcessor', { coord: e.coord });
    case 'forge_patcher_failed':
      return withDetailTail(
        translate('errors.forgePatcherFailed', { processor: e.processor }),
        e.details,
      );
    case 'forge_mappings_missing':
      return translate('errors.forgeMappingsMissing', { mc: e.mc });
    case 'instance_name_empty':
      return translate('errors.instanceNameEmpty');
    case 'instance_name_too_long':
      return translate('errors.instanceNameTooLong', { actual: e.actual, max: e.max });
    case 'mc_logs_upload':
      return withDetailTail(translate('errors.mcLogsUpload'), e.details);
    case 'mods_network':
      return translate('errors.modsNetwork');
    case 'mods_platform_auth':
      return e.kind_detail === 'invalid'
        ? translate('errors.modsPlatformAuthInvalid')
        : translate('errors.modsPlatformAuthMissing');
    case 'mods_platform_unreachable':
      return translate('errors.modsPlatformUnreachable');
    case 'mods_distribution_disabled':
      return translate('errors.modsDistributionDisabled', { source: e.source });
    case 'mods_not_found':
      return translate('errors.modsNotFound', { source: e.source });
    case 'mods_platform_unsupported':
      return translate('errors.modsPlatformUnsupported', { source: e.source });
    case 'mods_decode':
      return withDetailTail(translate('errors.modsDecode', { source: e.source }), e.details);
    case 'mods_sha1_unavailable':
      return translate('errors.modsSha1Unavailable');
    case 'mods_sha1_mismatch':
      // Wording intentionally omits the raw hashes — they're noise to a user;
      // the full hex lives in the launcher log.
      return translate('errors.modsSha1Mismatch');
    case 'mods_dependency_unresolvable':
      return translate('errors.modsDependencyUnresolvable', { projectRef: e.project_ref });
    case 'mods_filename_conflict':
      return translate('errors.modsFilenameConflict', { filename: e.filename });
    case 'mods_unsafe_filename':
      return translate('errors.modsUnsafeFilename', { filename: e.filename });
    case 'mods_cache_io':
      return withDetailTail(translate('errors.modsCacheIo'), e.details);
    case 'mods_instance_path':
      return withDetailTail(translate('errors.modsInstancePath', { path: e.path }), e.details);
    case 'modpack_invalid_archive':
      return withDetailTail(translate('errors.modpackInvalidArchive'), e.details);
    case 'modpack_format_unknown':
      return translate('errors.modpackFormatUnknown');
    case 'import_url_invalid':
      return translate('errors.importUrlInvalid', { reason: e.reason });
    case 'import_url_unsupported_source':
      return translate('errors.importUrlUnsupportedSource', { platform: e.platform });
    case 'modpack_manifest_invalid':
      return withDetailTail(
        translate('errors.modpackManifestInvalid', { format: e.format }),
        e.details,
      );
    case 'modpack_unsupported_manifest_version':
      return translate('errors.modpackUnsupportedManifestVersion', {
        format: e.format,
        version: e.version,
      });
    case 'modpack_unsupported_loader':
      return translate('errors.modpackUnsupportedLoader', {
        format: e.format,
        loaderId: e.loader_id,
      });
    case 'modpack_download_host_not_allowed':
      return translate('errors.modpackDownloadHostNotAllowed', {
        filePath: e.file_path,
        host: e.host,
      });
    case 'modpack_sha1_unavailable':
      return translate('errors.modpackSha1Unavailable', { modName: e.mod_name });
    case 'modpack_mod_distribution_disabled':
      return translate('errors.modpackModDistributionDisabled', {
        modName: e.mod_name,
        projectUrl: e.project_url,
      });
    case 'modpack_overrides_path_escape':
      return translate('errors.modpackOverridesPathEscape', { entry: e.entry });
    case 'modpack_overrides_too_large':
      return translate('errors.modpackOverridesTooLarge', {
        entry: e.entry,
        size: e.size,
        cap: e.cap,
      });
    case 'modpack_no_files_selected':
      return translate('errors.modpackNoFilesSelected');
    case 'modpack_instance_creation_failed':
      return withDetailTail(translate('errors.modpackInstanceCreationFailed'), e.details);
    case 'modpack_partial_failure':
      return translate('errors.modpackPartialFailure', { count: e.failed.length });
    case 'modpack_bundled_no_url':
      return translate('errors.modpackBundledNoUrl', { modName: e.mod_name });
    case 'modpack_cf_distribution_disabled':
      return translate('errors.modpackCfDistributionDisabled', { packName: e.pack_name });
    case 'modpack_export_failed':
      return withDetailTail(translate('errors.modpackExportFailed'), e.details);
    case 'world_not_found':
      return translate('errors.worldNotFound', { folderName: e.folder_name });
    case 'world_in_use':
      return translate('errors.worldInUse', { folderName: e.folder_name });
    case 'world_path_invalid':
      return translate('errors.worldPathInvalid', { name: e.name, reason: e.reason });
    case 'world_name_unresolvable':
      return translate('errors.worldNameUnresolvable', { folderName: e.folder_name });
    case 'screenshot_not_found':
      return translate('errors.screenshotNotFound', { filename: e.filename });
    case 'screenshot_path_invalid':
      return translate('errors.screenshotPathInvalid', { name: e.name, reason: e.reason });
    case 'backup_not_found':
      return translate('errors.backupNotFound', { filename: e.filename });
    case 'backup_corrupt':
      return withDetailTail(translate('errors.backupCorrupt', { filename: e.filename }), e.details);
    case 'world_import_not_a_world':
      return translate('errors.worldImportNotAWorld');
    case 'world_import_unsupported_source':
      return translate('errors.worldImportUnsupportedSource');
    case 'world_import_invalid_archive':
      return withDetailTail(translate('errors.worldImportInvalidArchive'), e.details);
    case 'world_import_too_large':
      return translate('errors.worldImportTooLarge');
    case 'playtime_io':
      return withDetailTail(translate('errors.playtimeIo'), e.details);
    case 'tray_io':
      return withDetailTail(translate('errors.trayIo'), e.details);
    case 'window_io':
      return withDetailTail(translate('errors.windowIo'), e.details);
    case 'update_check_failed':
      return withDetailTail(translate('errors.updateCheckFailed'), e.details);
    case 'update_verification_failed':
      return withDetailTail(translate('errors.updateVerificationFailed'), e.details);
    case 'update_install_failed':
      return withDetailTail(translate('errors.updateInstallFailed'), e.details);
    case 'quick_play_address_invalid':
      return translate('errors.quickPlayAddressInvalid', {
        address: e.address,
        reason: e.reason,
      });
    case 'import_instance_unreadable':
      return withDetailTail(
        translate('errors.importInstanceUnreadable', { launcher: e.launcher }),
        e.details,
      );
    case 'import_unsupported_loader':
      return translate('errors.importUnsupportedLoader', { loader: e.loader });
    case 'import_source_unrecognized':
      return translate('errors.importSourceUnrecognized', { path: e.path });
    case 'import_no_provenance':
      return translate('errors.importNoProvenance', { id: e.id });
    case 'import_source_missing':
      return translate('errors.importSourceMissing', { path: e.path });
    case 'servers_dat_parse':
      // `reason` is a raw fastnbt parse-error dump in two of the construction
      // sites — Opaque, so it truncates and points at Logs rather than leaking
      // English library text into the UI.
      return withDetailTail(translate('errors.serversDatParse'), e.reason);
    case 'level_dat_parse':
      // `reason` is a raw fastnbt/gzip message — Opaque, so it truncates and
      // points at Logs rather than leaking English library text into the UI.
      return withDetailTail(translate('errors.levelDatParse'), e.reason);
    case 'saved_server_name_invalid':
      return translate('errors.savedServerNameInvalid', { name: e.name, reason: e.reason });
    case 'saved_server_list_changed':
      return translate('errors.savedServerListChanged');
    case 'server_invalid_property':
      return translate('errors.serverInvalidProperty', { key: e.key, value: e.value });
    case 'server_eula_not_accepted':
      return translate('errors.serverEulaNotAccepted');
    case 'server_jar_unavailable':
      return translate('errors.serverJarUnavailable', {
        loader: e.loader,
        mcVersion: e.mc_version,
      });
    case 'server_installer_failed':
      return withDetailTail(
        translate('errors.serverInstallerFailed', { loader: e.loader }),
        e.details,
      );
    case 'server_spawn_failed':
      return withDetailTail(translate('errors.serverSpawnFailed'), e.details);
    case 'server_already_running':
      return translate('errors.serverAlreadyRunning');
    case 'server_upload_in_progress':
      return translate('errors.serverUploadInProgress');
    case 'upload_cancelled':
      return translate('errors.uploadCancelled');
    case 'server_not_running':
      return translate('errors.serverNotRunning');
    case 'server_name_invalid':
      return translate('errors.serverNameInvalid');
    case 'server_mod_required_by_other':
      return translate('errors.serverModRequiredByOther', {
        filename: e.filename,
        requiredBy: e.required_by,
      });
    case 'server_file_invalid':
      return translate('errors.serverFileInvalid', {
        filename: e.filename,
        reason: e.reason,
      });
    case 'server_core_unsupported':
      return translate('errors.serverCoreUnsupported', { reason: e.reason });
    case 'server_content_stale':
      return translate('errors.serverContentStale');
    case 'server_import_unsupported_source':
      return translate('errors.serverImportUnsupportedSource');
    case 'server_import_invalid_archive':
      return withDetailTail(translate('errors.serverImportInvalidArchive'), e.details);
    case 'server_import_too_large':
      return translate('errors.serverImportTooLarge');
    case 'server_import_not_a_server':
      return translate('errors.serverImportNotAServer');
    case 'server_import_staging_expired':
      return translate('errors.serverImportStagingExpired');
    case 'upload_not_configured':
      return translate('errors.uploadNotConfigured');
    case 'sftp_connect_failed':
      return withDetailTail(translate('errors.sftpConnectFailed'), e.details);
    case 'sftp_auth_failed':
      return translate('errors.sftpAuthFailed');
    case 'sftp_host_key_mismatch':
      return translate('errors.sftpHostKeyMismatch');
    case 'sftp_transfer_failed':
      return withDetailTail(translate('errors.sftpTransferFailed'), e.details);
    case 'data_location_busy':
      return translate('errors.dataLocationBusy');
    case 'data_location_invalid':
      return translate('errors.dataLocationInvalid', {
        reason: dataLocationInvalidReason(e.reason),
      });
    case 'data_location_migration_failed':
      return withDetailTail(translate('errors.dataLocationMigrationFailed'), e.reason);
    case 'data_location_unavailable':
      return translate('errors.dataLocationUnavailable');
    case 'changelog_unsupported':
      return translate('errors.changelogUnsupported');
    case 'datapack_invalid':
      return translate('errors.datapackInvalid', {
        filename: e.filename,
        reason: translate(datapackRejectionKey(e.reason)),
      });
    case 'l10n_translation_invalid':
      return translate('errors.l10nTranslationInvalid', {
        key: e.key,
        reason: formatErrorReason(e.reason),
      });
    case 'l10n_format_unknown':
      return translate('errors.l10nFormatUnknown', { mcVersion: e.mc_version });
    case 'l10n_format_too_old':
      return translate('errors.l10nFormatTooOld', { mcVersion: e.mc_version });
    default: {
      // Exhaustiveness guard. If a new Error variant lands in bindings.ts
      // without a case above, TypeScript will complain about the type of
      // `_exhaustive` (never vs the unhandled variant shape).
      const _exhaustive: never = e;
      return JSON.stringify(_exhaustive);
    }
  }
}
