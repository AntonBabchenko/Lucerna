// Pure routing decision for the app's SINGLE window-level drag-drop listener
// (`listenForFileDrops`, mounted by +page.svelte). Extracted so the matrix
// (open surfaces x mode x tab x kind x extension) is unit-testable without a
// webview. `planDrop` is the one table: the listener asks it on a drag's
// `enter` (what the drop box shows) and again on the `drop` (what happens), so
// the preview can never promise what the drop will not do. `routeDrop` is its
// route alone; the caller translates the target into the matching rune write.
import type { ContentKind } from '$lib/ipc/bindings';
import type { InstanceContentKind } from '$lib/mods/content-kind';
import type { ServerAddonsKind } from '$lib/settings/state.svelte';

export type DropContext = {
  // Only the topmost surface takes a drop (DESIGN.md §14). A dialog that takes
  // no files — Settings, Manage, a mod's details, the skin editor, a confirm —
  // covers the whole window under its scrim, the surfaces below included: while
  // it is the topmost modal nothing takes a drop, whatever popover or tour sits
  // over it (`modalBlocksFileDrops()`, layer-stack.svelte.ts).
  modalOnTop: boolean;
  // A surface on top owns every drop while it is up: the Modpacks modal covers
  // the whole window; the server-import view covers its servers panel — which
  // stays mounted, hidden, in client mode, so there it owns nothing.
  modpacksOpen: boolean;
  serverImportOpen: boolean;
  // The configured data folder is unavailable and the launcher runs on a
  // temporary one: a world or a modpack import would write into the wrong
  // place, so their boxes take nothing (data-root-gating.ts).
  dataRootFellBack: boolean;
  mode: 'client' | 'servers';
  // Tab ids arrive as plain strings on purpose: the router predates the
  // ServerTab union change and must not import either tab union — callers
  // pass their real union values. `clientTab` is null while MainTabs is not
  // mounted (compact mode unmounts the whole content column).
  clientTab: string | null;
  addonsKind: ContentKind;
  canInstallMods: boolean;
  instanceSelected: boolean;
  serversTab: string;
  serverAddonsKind: ServerAddonsKind | null;
  serverCanMutate: boolean;
};

/** A client add-on kind that installs from a `.zip`. */
type AssetKind = Exclude<InstanceContentKind, 'mod'>;

/** The drop box on screen that takes an OS file drop — the one a drag lights up. */
export type DropHost =
  | { target: 'modpack' }
  | { target: 'server-import' }
  | { target: 'client-world' }
  | { target: 'client-mods' }
  | { target: 'client-assets'; kind: AssetKind }
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

/** Why a dragged file is not added. The listener words each one; the blocked
 *  ones in the words the box's own disabled strip shows. */
export type DropSkip =
  // Nothing on screen takes files.
  | 'nowhere'
  // A dialog that takes no files is on top of whatever would.
  | 'modal'
  // The box is there but takes nothing right now.
  | 'no_instance'
  | 'no_mod_loader'
  | 'server_running'
  | 'data_root'
  // The box takes other files.
  | 'only_mods'
  | 'only_plugins'
  | 'only_resource_packs'
  | 'only_shaders'
  | 'only_datapacks'
  | 'only_modpacks'
  // The box takes one source per drop.
  | 'one_modpack'
  | 'one_server';

/** What a drop of `paths` does in a context. */
export type DropPlan = {
  /** The drop box the drag lights up; null when nothing on screen takes files. */
  host: DropHost | null;
  /** Where the files that are added go; null when none is. */
  route: DropRoute;
  /** Every other dragged file, and why it is not added. */
  skipped: { path: string; why: DropSkip }[];
};

/** Which drop box takes a drop in `ctx`, or null when nothing on screen takes files. Only the
 *  topmost surface can: a dialog that takes no files leaves everything under it out; a surface
 *  that owns drops comes next; under it, the mode's active tab (and kind) decides. */
export function dropHost(ctx: DropContext): DropHost | null {
  if (ctx.modalOnTop) return null;
  if (ctx.modpacksOpen) return { target: 'modpack' };
  if (ctx.mode === 'servers') {
    if (ctx.serverImportOpen) return { target: 'server-import' };
    if (ctx.serversTab !== 'addons' || ctx.serverAddonsKind === null) return null;
    return { target: 'server-content', kind: ctx.serverAddonsKind };
  }
  if (ctx.clientTab === 'worlds') return { target: 'client-world' };
  if (ctx.clientTab !== 'mod_browser') return null;
  if (ctx.addonsKind === 'mod') return { target: 'client-mods' };
  // Plugins install to servers; the client Add-ons tab never shows the kind.
  if (ctx.addonsKind === 'plugin') return null;
  return { target: 'client-assets', kind: ctx.addonsKind };
}

/** Why a box takes nothing right now (its strip is disabled for the same reason), or null. */
function blockOf(host: DropHost, ctx: DropContext): DropSkip | null {
  switch (host.target) {
    case 'modpack':
      return ctx.dataRootFellBack ? 'data_root' : null;
    case 'server-import':
      return null;
    case 'server-content':
      return ctx.serverCanMutate ? null : 'server_running';
    case 'client-world':
      if (!ctx.instanceSelected) return 'no_instance';
      return ctx.dataRootFellBack ? 'data_root' : null;
    case 'client-mods':
      return ctx.canInstallMods ? null : 'no_mod_loader';
    case 'client-assets':
      return ctx.instanceSelected ? null : 'no_instance';
  }
}

const isJar = (p: string) => p.toLowerCase().endsWith('.jar');
const isZip = (p: string) => p.toLowerCase().endsWith('.zip');
const isModpack = (p: string) => /\.(mrpack|zip)$/i.test(p);

const ASSET_ONLY: Record<AssetKind, DropSkip> = {
  resource_pack: 'only_resource_packs',
  shader: 'only_shaders',
  datapack: 'only_datapacks',
};

/** The files a box takes, and why it leaves the others; null when it takes any path — a world or
 *  a server source may be a folder, whose name says nothing (the importer judges it). */
function acceptOf(host: DropHost): { takes: (path: string) => boolean; other: DropSkip } | null {
  switch (host.target) {
    case 'modpack':
      return { takes: isModpack, other: 'only_modpacks' };
    case 'server-import':
    case 'client-world':
      return null;
    case 'client-mods':
      return { takes: isJar, other: 'only_mods' };
    case 'client-assets':
      return { takes: isZip, other: ASSET_ONLY[host.kind] };
    case 'server-content':
      if (host.kind === 'datapack') return { takes: isZip, other: 'only_datapacks' };
      return { takes: isJar, other: host.kind === 'plugin' ? 'only_plugins' : 'only_mods' };
  }
}

export function planDrop(paths: string[], ctx: DropContext): DropPlan {
  const host = dropHost(ctx);
  const skip = (list: string[], why: DropSkip) => list.map((path) => ({ path, why }));
  if (host === null)
    return { host, route: null, skipped: skip(paths, ctx.modalOnTop ? 'modal' : 'nowhere') };
  const block = blockOf(host, ctx);
  if (block !== null) return { host, route: null, skipped: skip(paths, block) };
  const accept = acceptOf(host);
  const taken = accept === null ? paths : paths.filter(accept.takes);
  const skipped =
    accept === null
      ? []
      : skip(
          paths.filter((p) => !accept.takes(p)),
          accept.other,
        );
  if (taken.length === 0) return { host, route: null, skipped };
  switch (host.target) {
    case 'modpack':
    case 'server-import': {
      const [first, ...rest] = taken;
      const one: DropSkip = host.target === 'modpack' ? 'one_modpack' : 'one_server';
      return {
        host,
        route: { target: host.target, path: first },
        skipped: [...skipped, ...skip(rest, one)],
      };
    }
    case 'client-world':
    case 'client-mods':
      return { host, route: { target: host.target, paths: taken }, skipped };
    case 'client-assets':
    case 'server-content':
      return { host, route: { ...host, paths: taken }, skipped };
  }
}

export function routeDrop(paths: string[], ctx: DropContext): DropRoute {
  return planDrop(paths, ctx).route;
}
