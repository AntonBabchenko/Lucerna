import { get } from 'svelte/store';
import { offlineNameRejectionKey } from '$lib/accounts/offline-name';
import { t } from '$lib/i18n';
import { displayLoader } from '$lib/instances/loader-display';
import type { Error as IpcError, LoaderKind } from '$lib/ipc/bindings';

// Detail-bearing errors are truncated to this many code points in the UI; the
// full text lives in the launcher log. Slicing is by code point (spread), not
// UTF-16 unit, so a surrogate pair at the boundary is never split.
const DETAIL_TRUNCATE_CODE_POINTS = 120;

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
      return translate('errors.serversDatParse', { reason: e.reason });
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
    case 'server_not_running':
      return translate('errors.serverNotRunning');
    case 'server_name_invalid':
      return translate('errors.serverNameInvalid');
    case 'server_mod_required_by_other':
      return translate('errors.serverModRequiredByOther', {
        filename: e.filename,
        requiredBy: e.required_by,
      });
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
    default: {
      // Exhaustiveness guard. If a new Error variant lands in bindings.ts
      // without a case above, TypeScript will complain about the type of
      // `_exhaustive` (never vs the unhandled variant shape).
      const _exhaustive: never = e;
      return JSON.stringify(_exhaustive);
    }
  }
}
