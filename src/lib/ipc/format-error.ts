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
    case 'hash_mismatch':
      return `Hash mismatch for ${e.path}`;
    case 'java_spawn':
      return `Java spawn failed: ${e.details}`;
    case 'already_running':
      return 'Minecraft is already running';
    case 'account_not_set':
      return 'Account not set — enter your name first';
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
    case 'io':
      return `IO error at ${e.path}: ${e.details}`;
    case 'forge_promotions_unavailable':
      return `Forge promotions feed for ${e.flavor} is unavailable — versions will not be marked recommended`;
    case 'forge_maven_metadata_parse_failed':
      return `Failed to parse Forge maven-metadata.xml: ${e.details}`;
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
    default: {
      // Exhaustiveness guard. If a new Error variant lands in bindings.ts
      // without a case above, TypeScript will complain about the type of
      // `_exhaustive` (never vs the unhandled variant shape).
      const _exhaustive: never = e;
      return JSON.stringify(_exhaustive);
    }
  }
}
