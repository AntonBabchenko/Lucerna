import type { ContentKind } from '$lib/ipc/bindings';
import { canInstallMods } from './install-eligibility';

type Loader = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;

export const CONTENT_KINDS: ContentKind[] = ['mod', 'resource_pack', 'shader'];

/** Mods need a non-vanilla instance; resource packs/shaders run on any
 *  selected instance (including vanilla). */
export function canInstallContent(
  kind: ContentKind,
  instanceId: string | null,
  loader: Loader,
): boolean {
  if (kind === 'mod') return canInstallMods(instanceId, loader);
  return instanceId !== null;
}
