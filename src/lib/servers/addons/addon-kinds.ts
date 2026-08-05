import type { ServerCore } from '$lib/ipc/bindings';
import { modCapable, pluginCapable } from '$lib/servers/core-display';
import type { ServerAddonsKind } from '$lib/settings/state.svelte';

/**
 * The content kinds this server's core and Minecraft version can take.
 *
 * Can be EMPTY — a pre-1.13 vanilla server takes neither mods nor plugins nor
 * datapacks. The caller must render an explanatory empty state rather than
 * indexing `[0]`.
 */
export function kindsFor(core: ServerCore, supportsDatapacks: boolean): ServerAddonsKind[] {
  const out: ServerAddonsKind[] = [];
  if (modCapable(core)) out.push('mod');
  if (pluginCapable(core)) out.push('plugin');
  if (supportsDatapacks) out.push('datapack');
  return out;
}
