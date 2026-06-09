import type { MissingModState } from '$lib/ipc/bindings';

/**
 * A `missing_mods` row still needs the user's attention unless the pinned
 * file is installed (`installed`) or the user installed a substitute from
 * another source (`substituted`). Centralised so the drawer count, the
 * Overview indicator, and the attention list never drift apart.
 */
export function isUnresolvedMissingState(state: MissingModState): boolean {
  return state !== 'installed' && state !== 'substituted';
}
