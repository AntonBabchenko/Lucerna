import type { Error as IpcError } from '$lib/ipc/bindings';

/**
 * Map a failed CurseForge key-validation error to the form's status pill.
 *
 * A reachability failure (region/Cloudflare block, network, disallowed host)
 * tells us nothing about the key — surface it as 'unverified' rather than
 * falsely claiming the key is invalid. Only a genuine platform-auth rejection
 * is 'invalid'.
 */
export function cfKeyErrorStatus(error: IpcError): 'invalid' | 'unverified' {
  switch (error.kind) {
    case 'mods_platform_unreachable':
    case 'mods_network':
    case 'host_not_allowed':
      return 'unverified';
    default:
      return 'invalid';
  }
}
