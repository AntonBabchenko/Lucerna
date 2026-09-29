// Pure routing decision for the app's SINGLE window-level drag-drop listener
// (`listenForFileDrops`, mounted by +page.svelte). Extracted so the matrix
// (open surfaces x mode x tab x kind x extension) is unit-testable without a
// webview. The caller translates the returned target into the matching rune
// write.
import type { ContentKind } from '$lib/ipc/bindings';
import type { ServerAddonsKind } from '$lib/settings/state.svelte';

export type DropContext = {
  // A surface on top owns every drop while it is up (DESIGN.md §14): the
  // Modpacks modal covers the whole window; the server-import view covers its
  // servers panel — which stays mounted, hidden, in client mode, so there it
  // owns nothing.
  modpacksOpen: boolean;
  serverImportOpen: boolean;
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

/** The drop box on screen that takes an OS file drop — the one a drag lights up. */
export type DropHost =
  | { target: 'modpack' }
  | { target: 'server-import' }
  | { target: 'client-world' }
  | { target: 'client-mods' }
  | { target: 'client-assets'; kind: ContentKind }
  | { target: 'server-content'; kind: ServerAddonsKind };

export type DropTarget = DropHost['target'];

export type DropRoute =
  // The Modpacks modal and the server-import view take one source per drop.
  | { target: 'modpack'; path: string }
  | { target: 'server-import'; path: string }
  | { target: 'client-world'; paths: string[] }
  | { target: 'client-mods'; paths: string[] }
  | { target: 'client-assets'; kind: ContentKind; paths: string[] }
  | { target: 'server-content'; kind: ServerAddonsKind; paths: string[] }
  | null;

/** Which drop box takes a drop in `ctx`, or null when nothing on screen takes files. A surface on
 *  top comes first; under it, the mode's active tab (and kind) decides. */
export function dropHost(ctx: DropContext): DropHost | null {
  if (ctx.modpacksOpen) return { target: 'modpack' };
  if (ctx.mode === 'servers') {
    if (ctx.serverImportOpen) return { target: 'server-import' };
    if (ctx.serversTab !== 'addons' || ctx.serverAddonsKind === null) return null;
    return { target: 'server-content', kind: ctx.serverAddonsKind };
  }
  if (ctx.clientTab === 'worlds') return { target: 'client-world' };
  if (ctx.clientTab !== 'mod_browser') return null;
  return ctx.addonsKind === 'mod'
    ? { target: 'client-mods' }
    : { target: 'client-assets', kind: ctx.addonsKind };
}

const byExt = (paths: string[], ext: string) => paths.filter((p) => p.toLowerCase().endsWith(ext));
const isModpack = (p: string) => /\.(mrpack|zip)$/i.test(p);

export function routeDrop(paths: string[], ctx: DropContext): DropRoute {
  if (paths.length === 0) return null;
  const host = dropHost(ctx);
  if (host === null) return null;
  switch (host.target) {
    case 'modpack': {
      const pack = paths.find(isModpack);
      return pack === undefined ? null : { target: 'modpack', path: pack };
    }
    case 'server-import':
      // A .zip or a server folder: the view inspects the source and says what it is.
      return { target: 'server-import', path: paths[0] };
    case 'server-content': {
      if (!ctx.serverCanMutate) return null;
      const matched = byExt(paths, host.kind === 'datapack' ? '.zip' : '.jar');
      return matched.length === 0 ? null : { ...host, paths: matched };
    }
    case 'client-world':
      return ctx.instanceSelected ? { target: 'client-world', paths } : null;
    case 'client-mods': {
      const jars = byExt(paths, '.jar');
      return jars.length > 0 && ctx.canInstallMods ? { target: 'client-mods', paths: jars } : null;
    }
    case 'client-assets': {
      const zips = byExt(paths, '.zip');
      return zips.length > 0 && ctx.instanceSelected ? { ...host, paths: zips } : null;
    }
  }
}
