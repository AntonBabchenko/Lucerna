// Pure routing decision for the app's SINGLE window-level drag-drop listener
// (owned by +page.svelte). Extracted so the matrix (mode x tab x kind x
// extension) is unit-testable without a webview. The caller translates the
// returned target into the matching rune write.
import type { ContentKind } from '$lib/ipc/bindings';
import type { ServerAddonsKind } from '$lib/settings/state.svelte';

export type DropContext = {
  mode: 'client' | 'servers';
  // Tab ids arrive as plain strings on purpose: the router predates the
  // ServerTab union change and must not import either tab union — callers
  // pass their real union values.
  clientTab: string;
  addonsKind: ContentKind;
  canInstallMods: boolean;
  instanceSelected: boolean;
  serversTab: string;
  serverAddonsKind: ServerAddonsKind | null;
  serverCanMutate: boolean;
};

export type DropRoute =
  | { target: 'client-world'; paths: string[] }
  | { target: 'client-mods'; paths: string[] }
  | { target: 'client-assets'; kind: ContentKind; paths: string[] }
  | { target: 'server-content'; kind: ServerAddonsKind; paths: string[] }
  | null;

const byExt = (paths: string[], ext: string) => paths.filter((p) => p.toLowerCase().endsWith(ext));

export function routeDrop(paths: string[], ctx: DropContext): DropRoute {
  if (paths.length === 0) return null;
  if (ctx.mode === 'servers') {
    if (ctx.serversTab !== 'addons' || ctx.serverAddonsKind === null || !ctx.serverCanMutate)
      return null;
    const wanted = ctx.serverAddonsKind === 'datapack' ? '.zip' : '.jar';
    const matched = byExt(paths, wanted);
    if (matched.length === 0) return null;
    return { target: 'server-content', kind: ctx.serverAddonsKind, paths: matched };
  }
  // Client mode — the former MainTabs rules, verbatim.
  if (ctx.clientTab === 'worlds') {
    return ctx.instanceSelected ? { target: 'client-world', paths } : null;
  }
  if (ctx.clientTab !== 'mod_browser') return null;
  if (ctx.addonsKind === 'mod') {
    const jars = byExt(paths, '.jar');
    return jars.length > 0 && ctx.canInstallMods ? { target: 'client-mods', paths: jars } : null;
  }
  const zips = byExt(paths, '.zip');
  return zips.length > 0 && ctx.instanceSelected
    ? { target: 'client-assets', kind: ctx.addonsKind, paths: zips }
    : null;
}
