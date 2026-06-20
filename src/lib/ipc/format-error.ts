import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { displayLoader } from '$lib/instances/loader-display';
import type { Error as IpcError, LoaderKind } from '$lib/ipc/bindings';

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
      return translate('errors.network', { url: e.url, details: e.details });
    case 'host_not_allowed':
      return translate('errors.hostNotAllowed', { url: e.url });
    case 'hash_mismatch':
      return translate('errors.hashMismatch', { path: e.path });
    case 'java_spawn':
      return translate('errors.javaSpawn', { details: e.details });
    case 'already_running':
      return translate('errors.alreadyRunning');
    case 'account_not_set':
      return translate('errors.accountNotSet');
    case 'instance_busy':
      return translate('errors.instanceBusy');
    case 'auth_cancelled':
      return translate('errors.authCancelled');
    case 'auth_failed':
      return translate('errors.authFailed', { stage: e.stage, details: e.details });
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
    case 'io': {
      // `details` can be a long parse-error dump (e.g. account.json contents
      // on a schema-mismatch). Toasts can't wrap 1000+ chars usefully —
      // truncate and point the user at the launcher log for the full text.
      // Slice by code points, not UTF-16 code units, so an emoji or other
      // surrogate-pair character at the boundary never gets split in half.
      const codePoints = [...e.details];
      const details =
        codePoints.length > 120
          ? `${codePoints.slice(0, 120).join('')}… (${translate('errors.ioTruncatedHint')})`
          : e.details;
      return translate('errors.io', { path: e.path, details });
    }
    case 'forge_promotions_unavailable':
      return translate('errors.forgePromotionsUnavailable', { flavor: e.flavor });
    case 'forge_maven_metadata_parse_failed':
      return translate('errors.forgeMavenMetadataParseFailed', { details: e.details });
    case 'forge_no_build_for':
      return translate('errors.forgeNoBuildFor', { mc: e.mc });
    case 'forge_installer_corrupted':
      return translate('errors.forgeInstallerCorrupted', {
        mc: e.mc,
        fv: e.fv,
        details: e.details,
      });
    case 'forge_unsupported_processor':
      return translate('errors.forgeUnsupportedProcessor', { coord: e.coord });
    case 'forge_patcher_failed':
      return translate('errors.forgePatcherFailed', { processor: e.processor, details: e.details });
    case 'forge_mappings_missing':
      return translate('errors.forgeMappingsMissing', { mc: e.mc });
    case 'instance_name_empty':
      return translate('errors.instanceNameEmpty');
    case 'instance_name_too_long':
      return translate('errors.instanceNameTooLong', { actual: e.actual, max: e.max });
    case 'mc_logs_upload':
      return translate('errors.mcLogsUpload', { details: e.details });
    case 'mods_network':
      return translate('errors.modsNetwork', { url: e.url, details: e.details });
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
      return translate('errors.modsDecode', { source: e.source, details: e.details });
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
      return translate('errors.modsCacheIo', { details: e.details });
    case 'mods_instance_path':
      return translate('errors.modsInstancePath', { path: e.path, details: e.details });
    case 'modpack_invalid_archive':
      return translate('errors.modpackInvalidArchive', { details: e.details });
    case 'modpack_format_unknown':
      return translate('errors.modpackFormatUnknown');
    case 'modpack_manifest_invalid':
      return translate('errors.modpackManifestInvalid', { format: e.format, details: e.details });
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
      return translate('errors.modpackInstanceCreationFailed', { details: e.details });
    case 'modpack_partial_failure':
      return translate('errors.modpackPartialFailure', { count: e.failed.length });
    case 'modpack_bundled_no_url':
      return translate('errors.modpackBundledNoUrl', { modName: e.mod_name });
    case 'modpack_cf_distribution_disabled':
      return translate('errors.modpackCfDistributionDisabled', { packName: e.pack_name });
    case 'modpack_export_failed':
      return translate('errors.modpackExportFailed', { details: e.details });
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
      return translate('errors.backupCorrupt', { filename: e.filename, details: e.details });
    case 'world_import_not_a_world':
      return translate('errors.worldImportNotAWorld');
    case 'world_import_unsupported_source':
      return translate('errors.worldImportUnsupportedSource');
    case 'world_import_invalid_archive':
      return translate('errors.worldImportInvalidArchive', { details: e.details });
    case 'world_import_too_large':
      return translate('errors.worldImportTooLarge');
    case 'playtime_io':
      return translate('errors.playtimeIo', { details: e.details });
    case 'tray_io':
      return translate('errors.trayIo', { details: e.details });
    case 'window_io':
      return translate('errors.windowIo', { details: e.details });
    case 'update_check_failed':
      return translate('errors.updateCheckFailed', { details: e.details });
    case 'update_verification_failed':
      return translate('errors.updateVerificationFailed', { details: e.details });
    case 'update_install_failed':
      return translate('errors.updateInstallFailed', { details: e.details });
    case 'quick_play_address_invalid':
      return translate('errors.quickPlayAddressInvalid', {
        address: e.address,
        reason: e.reason,
      });
    case 'import_instance_unreadable':
      return translate('errors.importInstanceUnreadable', {
        launcher: e.launcher,
        details: e.details,
      });
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
      return translate('errors.serverInstallerFailed', { loader: e.loader });
    case 'server_spawn_failed':
      return translate('errors.serverSpawnFailed', { details: e.details });
    case 'server_already_running':
      return translate('errors.serverAlreadyRunning');
    case 'server_not_running':
      return translate('errors.serverNotRunning');
    case 'server_mod_required_by_other':
      return translate('errors.serverModRequiredByOther', {
        filename: e.filename,
        requiredBy: e.required_by,
      });
    case 'server_import_unsupported_source':
      return translate('errors.serverImportUnsupportedSource');
    case 'server_import_invalid_archive':
      return translate('errors.serverImportInvalidArchive', { details: e.details });
    case 'server_import_too_large':
      return translate('errors.serverImportTooLarge');
    case 'server_import_not_a_server':
      return translate('errors.serverImportNotAServer');
    case 'server_import_staging_expired':
      return translate('errors.serverImportStagingExpired');
    case 'upload_not_configured':
      return translate('errors.uploadNotConfigured');
    case 'sftp_connect_failed':
      return translate('errors.sftpConnectFailed', { details: e.details });
    case 'sftp_auth_failed':
      return translate('errors.sftpAuthFailed');
    case 'sftp_host_key_mismatch':
      return translate('errors.sftpHostKeyMismatch');
    case 'sftp_transfer_failed':
      return translate('errors.sftpTransferFailed', { details: e.details });
    default: {
      // Exhaustiveness guard. If a new Error variant lands in bindings.ts
      // without a case above, TypeScript will complain about the type of
      // `_exhaustive` (never vs the unhandled variant shape).
      const _exhaustive: never = e;
      return JSON.stringify(_exhaustive);
    }
  }
}
