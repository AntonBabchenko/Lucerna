/**
 * Who takes an OS file drop (DESIGN.md §14). Tauri hands every drag-drop listener of the window
 * the same event, so a surface that registered its own listener while the page's was live took
 * the drop TOGETHER with it: a .zip dropped on the open Modpacks modal was imported as a modpack
 * AND installed into the Resource packs tab underneath, and a server-import view left mounted in
 * the hidden servers panel took every client drop for itself. One listener (`listenForFileDrops`)
 * and one router now decide: the surface on top owns the drop, and only its drop box lights up.
 */
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { DropContext } from '$lib/layout/drop-router';

// Every listener the window has, and a way to fire an event at all of them — what Tauri does.
const drag = vi.hoisted(() => {
  type Handler = (event: { payload: unknown }) => void;
  const handlers: Handler[] = [];
  return {
    handlers,
    emit(payload: unknown) {
      for (const h of [...handlers]) h({ payload });
    },
  };
});
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (h: (typeof drag.handlers)[number]) => {
      drag.handlers.push(h);
      return Promise.resolve(() => {
        const i = drag.handlers.indexOf(h);
        if (i >= 0) drag.handlers.splice(i, 1);
      });
    },
  }),
}));

const serverImport = vi.hoisted(() => ({
  importInspect: vi.fn(),
  importCommit: vi.fn(),
  importCancel: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('$lib/servers/server-state.svelte', () => ({ serverState: serverImport }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // ModpacksTab (as in tests/modpacks-tab.test.ts)
    modpackSearch: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, limit: 20 },
    }),
    modpackSourceCaps: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { needs_api_key: false, supports_server_filter: true, can_export: true },
    }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'present' }),
    modpackInspect: vi
      .fn()
      .mockResolvedValue({ status: 'error', error: { kind: 'modpack_format_unknown' } }),
    modpackImport: vi.fn(),
    modpackFetchToTemp: vi.fn(),
    getDataLocation: vi.fn(),
  },
  events: { gpuPrefApplied: { listen: () => Promise.resolve(() => {}) } },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: vi.fn() }));

import { commands } from '$lib/ipc/bindings';
import { listenForFileDrops } from '$lib/layout/window-drop';
import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
import FileDropzone from '$lib/mods/FileDropzone.svelte';
import ServerImportView from '$lib/servers/ServerImportView.svelte';
import {
  droppedAssets,
  droppedModpack,
  droppedMods,
  droppedServer,
  serverImportActive,
} from '$lib/settings/state.svelte';

// The page's context on the client Add-ons tab, Resource packs showing.
const addonsResourcePacks: DropContext = {
  modpacksOpen: false,
  serverImportOpen: false,
  dataRootFellBack: false,
  mode: 'client',
  clientTab: 'mod_browser',
  addonsKind: 'resource_pack',
  canInstallMods: true,
  instanceSelected: true,
  serversTab: 'overview',
  serverAddonsKind: null,
  serverCanMutate: false,
};

let stop: (() => void) | null = null;
const settle = () => new Promise((r) => setTimeout(r, 20));

beforeAll(() => locale.set('en'));
beforeEach(() => {
  drag.handlers.length = 0;
  vi.mocked(commands.modpackInspect).mockClear();
  serverImport.importInspect.mockReset();
  serverImport.importInspect.mockResolvedValue({ ok: false, error: { kind: 'io', message: 'x' } });
});
afterEach(() => {
  stop?.();
  stop = null;
  drag.emit({ type: 'leave' });
  droppedAssets.value = null;
  droppedModpack.value = null;
  droppedMods.value = null;
  droppedServer.value = null;
});

describe('the Modpacks modal owns the drop while it is open', () => {
  it('a .zip dropped on it is handled once — as a modpack, never also as an Add-ons asset', async () => {
    render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    stop = listenForFileDrops(() => ({ ...addonsResourcePacks, modpacksOpen: true }));
    await tick();

    drag.emit({ type: 'drop', paths: ['C:/packs/pack.zip'] });
    await settle();

    expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledWith('C:/packs/pack.zip');
    expect(droppedAssets.value).toBeNull();
  });

  it('only its own drop box lights up — the Add-ons overlay does not paint behind it', async () => {
    const modpacks = render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    // The Add-ons tab's strip underneath (Resource packs showing).
    const addons = render(FileDropzone, {
      props: {
        variant: 'strip',
        target: 'client-assets',
        label: 'Drop a resource pack .zip here',
        dragLabel: 'Drop to add to “P”',
        onClick: () => {},
      },
    });
    stop = listenForFileDrops(() => ({ ...addonsResourcePacks, modpacksOpen: true }));
    await tick();

    drag.emit({ type: 'enter', paths: ['C:/packs/pack.zip'] });
    await tick();

    const overlay = (r: { container: HTMLElement }) =>
      within(r.container).getByTestId('file-dropzone-overlay');
    expect(overlay(modpacks).className).toContain('opacity-100');
    expect(overlay(addons).className).toContain('opacity-0');
    expect(within(addons.container).getByTestId('file-dropzone').className).not.toContain(
      'bg-accent-soft',
    );
  });

  it('raises its flag while mounted and lowers it when it goes', async () => {
    const { modpacksActive } = await import('$lib/settings/state.svelte');
    const view = render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    expect(modpacksActive.value).toBe(true);
    view.unmount();
    expect(modpacksActive.value).toBe(false);
  });
});

describe('the server-import view owns the drop only where it can be seen', () => {
  it('in client mode a file reaches the client tab, not the import view left in the hidden servers panel', async () => {
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    await settle();
    expect(serverImportActive.value).toBe(true);
    stop = listenForFileDrops(() => ({
      ...addonsResourcePacks,
      addonsKind: 'mod',
      serverImportOpen: serverImportActive.value,
    }));

    drag.emit({ type: 'drop', paths: ['C:/mods/a.jar'] });
    await settle();

    expect(droppedMods.value).toEqual(['C:/mods/a.jar']);
    expect(serverImport.importInspect).not.toHaveBeenCalled();
  });

  it('with the Modpacks modal over it, a .zip is imported as a modpack only', async () => {
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    await settle();
    stop = listenForFileDrops(() => ({
      ...addonsResourcePacks,
      mode: 'servers',
      modpacksOpen: true,
      serverImportOpen: true,
    }));

    drag.emit({ type: 'drop', paths: ['C:/packs/pack.zip'] });
    await settle();

    expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledTimes(1);
    expect(serverImport.importInspect).not.toHaveBeenCalled();
  });
});

describe('one listener, one decision', () => {
  function sourceFiles(dir: string, acc: string[] = []): string[] {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) sourceFiles(full, acc);
      else if (/\.(svelte|ts)$/.test(entry.name)) acc.push(full);
    }
    return acc;
  }

  it('only listenForFileDrops listens to the window’s drag-drop events', () => {
    const listeners = sourceFiles('src').filter((f) =>
      readFileSync(f, 'utf8').includes('onDragDropEvent('),
    );
    expect(listeners).toEqual([join('src', 'lib', 'layout', 'window-drop.ts')]);
  });

  it('the page tells the router which surfaces are up', () => {
    const page = readFileSync(join('src', 'routes', '+page.svelte'), 'utf8');
    const start = page.indexOf('listenForFileDrops(');
    expect(start, 'the page mounts listenForFileDrops').toBeGreaterThan(-1);
    const call = page.slice(start, page.indexOf('}),', start));
    for (const wiring of [
      'modpacksOpen: modpacksActive.value',
      'serverImportOpen: serverImportActive.value',
    ])
      expect(call.includes(wiring), wiring).toBe(true);
  });
});
