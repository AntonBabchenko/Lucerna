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
  switch (e.kind) {
    case 'network':
      return `Network error fetching ${e.url}: ${e.details}`;
    case 'host_not_allowed':
      return `Network request to ${e.url} is not on the allowed-host list`;
    case 'hash_mismatch':
      return `Hash mismatch for ${e.path}`;
    case 'java_spawn':
      return `Java spawn failed: ${e.details}`;
    case 'already_running':
      return 'Minecraft is already running';
    case 'account_not_set':
      return 'Account not set — enter your name first';
    case 'auth_cancelled':
      return 'Microsoft sign-in cancelled.';
    case 'auth_failed':
      return `Microsoft sign-in failed at ${e.stage}: ${e.details}`;
    case 'no_minecraft_profile':
      return "This Microsoft account doesn't own Minecraft. Sign in with an account that owns a copy.";
    case 'auth_pending_approval':
      return "Microsoft hasn't approved Lucerna's app registration yet. This sign-in will work once approved. Use an offline account in the meantime.";
    case 'unknown_version':
      return `Version ${e.id} not found in manifest`;
    case 'unsupported_platform':
      return `Unsupported platform: ${e.os}/${e.arch}`;
    case 'loader_unavailable':
      return `${displayLoader(e.loader as LoaderKind)} does not support Minecraft ${e.mc_version}`;
    case 'last_instance':
      return 'Cannot delete the last instance — at least one must remain';
    case 'no_version_selected':
      return 'Pick a Minecraft version first';
    case 'instance_not_found':
      return `Instance ${e.id} not found`;
    case 'io': {
      // `details` can be a long parse-error dump (e.g. account.json contents
      // on a schema-mismatch). Toasts can't wrap 1000+ chars usefully —
      // truncate and point the user at the launcher log for the full text.
      // Slice by code points, not UTF-16 code units, so an emoji or other
      // surrogate-pair character at the boundary never gets split in half.
      const codePoints = [...e.details];
      const details =
        codePoints.length > 120
          ? `${codePoints.slice(0, 120).join('')}… (open Logs for full text)`
          : e.details;
      return `IO error at ${e.path}: ${details}`;
    }
    case 'forge_promotions_unavailable':
      return `Forge promotions feed for ${e.flavor} is unavailable — versions will not be marked recommended`;
    case 'forge_maven_metadata_parse_failed':
      return `Failed to parse Forge maven-metadata.xml: ${e.details}`;
    case 'forge_no_build_for':
      return `No Forge build exists for Minecraft ${e.mc} — pick a different Minecraft version or loader.`;
    case 'forge_installer_corrupted':
      return `Forge installer for ${e.mc}-${e.fv} is corrupted: ${e.details}`;
    case 'forge_unsupported_processor':
      return `This Forge version uses an unsupported processor: ${e.coord}`;
    case 'forge_patcher_failed':
      return `Forge patcher "${e.processor}" failed: ${e.details}`;
    case 'forge_mappings_missing':
      return `Forge mappings for ${e.mc} are not available`;
    case 'instance_name_empty':
      return 'Instance name cannot be empty';
    case 'instance_name_too_long':
      return `Instance name is too long: ${e.actual}/${e.max} characters`;
    case 'mc_logs_upload':
      return `Couldn't upload log to mclo.gs: ${e.details}`;
    case 'mods_network':
      return `Network error talking to ${e.url}: ${e.details}`;
    case 'mods_platform_auth':
      return e.kind_detail === 'invalid'
        ? 'CurseForge API key is invalid — please enter a new one in Settings'
        : 'CurseForge requires an API key — set it in Settings';
    case 'mods_distribution_disabled':
      return `This mod's author has disabled third-party launcher downloads on ${e.source}.`;
    case 'mods_not_found':
      return `This mod is no longer available on ${e.source}.`;
    case 'mods_decode':
      return `Unexpected response from ${e.source}: ${e.details}`;
    case 'mods_sha1_unavailable':
      return "This mod's hash is missing; refusing to install.";
    case 'mods_sha1_mismatch':
      return `Verification failed: expected ${e.expected}, got ${e.got}`;
    case 'mods_dependency_unresolvable':
      return `Required dependency ${e.project_ref} is not available for this MC + loader`;
    case 'mods_filename_conflict':
      return `A different file named "${e.filename}" already exists in this instance — uninstall it first`;
    case 'mods_cache_io':
      return `Couldn't write to mod cache: ${e.details}`;
    case 'mods_instance_path':
      return `Couldn't write to instance at ${e.path}: ${e.details}`;
    case 'modpack_invalid_archive':
      return `Modpack archive is invalid: ${e.details}`;
    case 'modpack_format_unknown':
      return 'This file is not a recognised modpack (.mrpack or CurseForge .zip).';
    case 'modpack_manifest_invalid':
      return `${e.format} modpack manifest is invalid: ${e.details}`;
    case 'modpack_unsupported_manifest_version':
      return `${e.format} modpack uses unsupported manifest version ${e.version}.`;
    case 'modpack_unsupported_loader':
      return `${e.format} modpack declares unsupported loader: ${e.loader_id}`;
    case 'modpack_download_host_not_allowed':
      return `Modpack file ${e.file_path} references ${e.host} which is not on the network allowlist.`;
    case 'modpack_sha1_unavailable':
      return `Modpack file ${e.mod_name} has no SHA-1 in the manifest — cannot verify integrity.`;
    case 'modpack_mod_distribution_disabled':
      return `${e.mod_name} cannot be auto-installed by third-party launchers. Download manually from ${e.project_url}.`;
    case 'modpack_overrides_path_escape':
      return `Modpack overrides entry tried to escape the instance directory: ${e.entry}`;
    case 'modpack_overrides_too_large':
      return `Modpack overrides entry ${e.entry} exceeds the safety cap (${e.size} bytes; cap is ${e.cap}).`;
    case 'modpack_no_files_selected':
      return 'Select at least one mod before importing.';
    case 'modpack_instance_creation_failed':
      return `Could not create instance for modpack: ${e.details}`;
    case 'modpack_partial_failure':
      return `Modpack imported with ${e.failed.length} mod(s) skipped — see the warning notification for details.`;
    case 'modpack_bundled_no_url':
      return `'${e.mod_name}' was bundled inside the .mrpack and cannot be restored automatically. Re-import the pack to recover it.`;
    case 'modpack_cf_distribution_disabled':
      return `"${e.pack_name}" cannot be downloaded by third-party launchers — its author disabled distribution. Open it on CurseForge to download the pack manually.`;
    case 'modpack_export_failed':
      return `Couldn't export the modpack: ${e.details}`;
    case 'world_not_found':
      return `World "${e.folder_name}" not found in this instance`;
    case 'world_in_use':
      return `World "${e.folder_name}" is currently in use — quit Minecraft and try again`;
    case 'world_path_invalid':
      return `Invalid name "${e.name}": ${e.reason}`;
    case 'world_name_unresolvable':
      return `Couldn't find a free name for "${e.folder_name}" — too many similarly-named copies exist`;
    case 'backup_not_found':
      return `Backup "${e.filename}" not found`;
    case 'backup_corrupt':
      return `Backup "${e.filename}" is unreadable or corrupted: ${e.details}`;
    case 'playtime_io':
      return `Couldn't read or write playtime stats: ${e.details}`;
    case 'tray_io':
      return `Couldn't show or hide the tray icon: ${e.details}`;
    case 'update_check_failed':
      return `Couldn't check for updates: ${e.details}`;
    case 'update_verification_failed':
      return `Update verification failed — the download may be corrupt or tampered with. ${e.details}`;
    case 'update_install_failed':
      return `Couldn't install the update: ${e.details}`;
    default: {
      // Exhaustiveness guard. If a new Error variant lands in bindings.ts
      // without a case above, TypeScript will complain about the type of
      // `_exhaustive` (never vs the unhandled variant shape).
      const _exhaustive: never = e;
      return JSON.stringify(_exhaustive);
    }
  }
}
