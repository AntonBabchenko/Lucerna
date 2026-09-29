/**
 * A drag says what its drop will do (DESIGN.md §14). The window drop router decides once per drag,
 * on `enter`, with the same plan the drop then carries out: a box the dragged files do not fit says
 * what it takes instead of promising to add them, and whatever a drop leaves behind is named in a
 * toast with the reason — a file is never dropped in silence.
 */
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

import { listenForFileDrops } from '$lib/layout/window-drop';
import FileDropzone from '$lib/mods/FileDropzone.svelte';
import { droppedModpack, droppedMods, droppedWorld } from '$lib/settings/state.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

// The page's context on the client Add-ons tab, Mods showing, a profile with a loader selected.
const mods: DropContext = {
  modalOnTop: false,
  modpacksOpen: false,
  serverImportOpen: false,
  dataRootFellBack: false,
  mode: 'client',
  clientTab: 'mod_browser',
  addonsKind: 'mod',
  canInstallMods: true,
  instanceSelected: true,
  serversTab: 'overview',
  serverAddonsKind: null,
  serverCanMutate: false,
};
let ctx: DropContext = mods;
let stop: () => void = () => {};

const ONLY_MODS = 'Only mod .jar files can be added here';
const warnings = () => toastList().filter((x) => x.kind === 'warning');

function modsBox(variant: 'strip' | 'full') {
  const r = render(FileDropzone, {
    props: {
      variant,
      target: 'client-mods',
      label: 'Drop a mod .jar here to install — or click to browse',
      dragLabel: 'Drop to add to “Pack”',
      onClick: () => {},
    },
  });
  return {
    box: () => within(r.container).getByTestId('file-dropzone'),
    overlay: () => within(r.container).getByTestId('file-dropzone-overlay'),
  };
}

beforeAll(() => locale.set('en'));
beforeEach(() => {
  drag.handlers.length = 0;
  ctx = mods;
  stop = listenForFileDrops(() => ctx);
});
afterEach(() => {
  drag.emit({ type: 'leave' });
  stop();
  droppedMods.value = null;
  droppedModpack.value = null;
  droppedWorld.value = null;
  for (const x of toastList()) dismiss(x.id);
});

describe('while a file is dragged', () => {
  it('a box the files do not fit makes no promise — its overlay says what it takes', async () => {
    const { box, overlay } = modsBox('strip');
    drag.emit({ type: 'enter', paths: ['C:/pics/shot.png'] });
    await tick();
    expect(overlay().className).toContain('opacity-100');
    expect(overlay().textContent).toContain(ONLY_MODS);
    expect(overlay().textContent).not.toContain('Drop to add');
    expect(overlay().className).not.toContain('border-accent');
    expect(box().className).not.toContain('bg-accent-soft');
  });

  it('a mixed drag promises what fits and says what will stay behind', async () => {
    const { box, overlay } = modsBox('strip');
    drag.emit({ type: 'enter', paths: ['C:/mods/a.jar', 'C:/pics/shot.png'] });
    await tick();
    expect(overlay().textContent).toContain('Drop to add to “Pack”');
    expect(overlay().textContent).toContain(ONLY_MODS);
    expect(overlay().className).toContain('border-accent');
    expect(box().className).toContain('bg-accent-soft');
  });

  it('a drag of files that all fit promises and adds no caveat', async () => {
    const { overlay } = modsBox('strip');
    drag.emit({ type: 'enter', paths: ['C:/mods/a.jar', 'C:/mods/b.jar'] });
    await tick();
    expect(overlay().textContent).toContain('Drop to add to “Pack”');
    expect(overlay().textContent).not.toContain(ONLY_MODS);
  });

  it('an empty list’s full box stays neutral for a file it does not take and says why', async () => {
    const { box } = modsBox('full');
    drag.emit({ type: 'enter', paths: ['C:/pics/shot.png'] });
    await tick();
    expect(box().className).not.toContain('bg-accent-soft');
    expect(box().className).not.toContain('border-accent');
    expect(box().textContent).toContain(ONLY_MODS);
  });
});

describe('when the files are dropped', () => {
  it('a file the box does not take is not dropped in silence — a toast says why', async () => {
    drag.emit({ type: 'drop', paths: ['C:/pics/shot.png'] });
    await tick();
    expect(droppedMods.value).toBeNull();
    expect(warnings()).toHaveLength(1);
    expect(warnings()[0].title).toBe("1 file wasn't added");
    expect(warnings()[0].lines).toEqual([`shot.png: ${ONLY_MODS}`]);
  });

  it('a mixed drop adds the files that fit and names the ones it skipped', async () => {
    drag.emit({
      type: 'drop',
      paths: ['C:/mods/a.jar', 'C:/pics/shot.png', 'C:\\docs\\readme.txt'],
    });
    await tick();
    expect(droppedMods.value).toEqual(['C:/mods/a.jar']);
    expect(warnings()).toHaveLength(1);
    expect(warnings()[0].title).toBe("2 files weren't added");
    expect(warnings()[0].lines).toEqual([`shot.png, readme.txt: ${ONLY_MODS}`]);
  });

  it('a drop of files that all fit adds them and says nothing more', async () => {
    drag.emit({ type: 'drop', paths: ['C:/mods/a.jar'] });
    await tick();
    expect(droppedMods.value).toEqual(['C:/mods/a.jar']);
    expect(warnings()).toHaveLength(0);
  });

  it('a box that takes nothing right now says why — the reason its strip shows', async () => {
    ctx = { ...mods, canInstallMods: false };
    drag.emit({ type: 'drop', paths: ['C:/mods/a.jar'] });
    await tick();
    expect(droppedMods.value).toBeNull();
    expect(warnings()[0].lines).toEqual(['a.jar: Select a non-vanilla instance to install mods']);
  });

  it('a drop where nothing on screen takes files says so', async () => {
    ctx = { ...mods, clientTab: 'overview' };
    drag.emit({ type: 'drop', paths: ['C:/mods/a.jar'] });
    await tick();
    expect(droppedMods.value).toBeNull();
    expect(warnings()[0].lines).toEqual(["a.jar: Files can't be added here"]);
  });

  it('the Modpacks modal imports one pack per drop and names the rest', async () => {
    ctx = { ...mods, modpacksOpen: true };
    drag.emit({ type: 'drop', paths: ['C:/a.mrpack', 'C:/b.zip', 'C:/shot.png'] });
    await tick();
    expect(droppedModpack.value).toBe('C:/a.mrpack');
    expect(warnings()[0].title).toBe("2 files weren't added");
    expect(warnings()[0].lines).toEqual([
      'shot.png: Only modpacks (.mrpack or .zip) can be imported here',
      'b.zip: Modpacks are imported one at a time',
    ]);
  });

  it('while the data folder is unavailable a world is not added, and the toast says why', async () => {
    ctx = { ...mods, clientTab: 'worlds', dataRootFellBack: true };
    drag.emit({ type: 'drop', paths: ['C:/maps/World.zip'] });
    await tick();
    expect(droppedWorld.value).toBeNull();
    expect(warnings()[0].lines).toEqual([
      'World.zip: Creating and moving instance data is disabled while the data folder is unavailable.',
    ]);
  });

  it('a drag with no files in it (not a file drag) lights nothing and says nothing', async () => {
    const { overlay } = modsBox('strip');
    drag.emit({ type: 'enter', paths: [] });
    await tick();
    expect(overlay().className).toContain('opacity-0');
    drag.emit({ type: 'drop', paths: [] });
    await tick();
    expect(warnings()).toHaveLength(0);
  });
});
