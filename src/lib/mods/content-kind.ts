import type { ContentKind } from '$lib/ipc/bindings';
import { canInstallMods } from './install-eligibility';

type Loader = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;

// Instance-installable kinds only — "plugin" (Bukkit/Paper/Purpur) attaches to
// servers, not client instances, so it's deliberately excluded here (see
// servers/mods/ServerModBrowser and the plugin management surface). The alias
// keeps the exclusion compiler-enforced at every instance-side call site.
export type InstanceContentKind = Exclude<ContentKind, 'plugin'>;

export const CONTENT_KINDS: InstanceContentKind[] = ['mod', 'resource_pack', 'shader', 'datapack'];

/** Mods need a non-vanilla instance; resource packs/shaders/datapacks run on
 *  any selected instance (including vanilla — datapacks deliberately have NO
 *  loader gate; the separate 1.13+ version gate lives in
 *  `instance_supports_datapacks` and hides the whole kind, not the button). */
export function canInstallContent(
  kind: InstanceContentKind,
  instanceId: string | null,
  loader: Loader,
): boolean {
  if (kind === 'mod') return canInstallMods(instanceId, loader);
  return instanceId !== null;
}
