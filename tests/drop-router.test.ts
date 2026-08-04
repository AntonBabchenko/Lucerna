import { describe, expect, it } from 'vitest';
import { type DropContext, routeDrop } from '$lib/layout/drop-router';

const base: DropContext = {
  mode: 'client',
  clientTab: 'mod_browser',
  addonsKind: 'mod',
  canInstallMods: true,
  instanceSelected: true,
  serversTab: 'addons',
  serverAddonsKind: null,
  serverCanMutate: true,
};

describe('routeDrop', () => {
  it('routes .jar drops to droppedMods on the client add-ons tab (mod kind)', () => {
    expect(routeDrop(['C:/a.jar', 'C:/b.txt'], base)).toEqual({
      target: 'client-mods',
      paths: ['C:/a.jar'],
    });
  });

  it('routes .zip drops to droppedAssets for non-mod client kinds', () => {
    expect(routeDrop(['C:/p.zip'], { ...base, addonsKind: 'resource_pack' })).toEqual({
      target: 'client-assets',
      kind: 'resource_pack',
      paths: ['C:/p.zip'],
    });
  });

  it('routes .zip drops on the datapack kind through the same client-assets target', () => {
    // The router deliberately does NOT special-case datapacks: the kind rides
    // along in the payload, and AddonsTab's droppedAssets consumer forks it to
    // the datapack LIBRARY install instead of the asset pipeline. This pin is
    // what that consumer's contract stands on.
    expect(
      routeDrop(['C:/terralith.zip', 'C:/readme.txt'], { ...base, addonsKind: 'datapack' }),
    ).toEqual({
      target: 'client-assets',
      kind: 'datapack',
      paths: ['C:/terralith.zip'],
    });
  });

  it('routes everything to droppedWorld on the worlds tab', () => {
    expect(routeDrop(['C:/w'], { ...base, clientTab: 'worlds' })).toEqual({
      target: 'client-world',
      paths: ['C:/w'],
    });
  });

  it('ignores drops on client tabs that take none', () => {
    expect(routeDrop(['C:/a.jar'], { ...base, clientTab: 'overview' })).toBeNull();
  });

  it('servers mode: routes by the active server add-ons kind and extension', () => {
    const ctx: DropContext = { ...base, mode: 'servers', serverAddonsKind: 'mod' };
    expect(routeDrop(['C:/a.jar'], ctx)).toEqual({
      target: 'server-content',
      kind: 'mod',
      paths: ['C:/a.jar'],
    });
    expect(routeDrop(['C:/d.zip'], { ...ctx, serverAddonsKind: 'datapack' })).toEqual({
      target: 'server-content',
      kind: 'datapack',
      paths: ['C:/d.zip'],
    });
  });

  it('servers mode: null when Add-ons is not the active tab or kind is unset', () => {
    expect(
      routeDrop(['C:/a.jar'], { ...base, mode: 'servers', serversTab: 'overview' }),
    ).toBeNull();
    expect(
      routeDrop(['C:/a.jar'], { ...base, mode: 'servers', serverAddonsKind: null }),
    ).toBeNull();
  });

  it('servers mode: extension mismatch for the kind yields null', () => {
    expect(
      routeDrop(['C:/a.zip'], { ...base, mode: 'servers', serverAddonsKind: 'plugin' }),
    ).toBeNull();
  });

  it('respects gating flags', () => {
    expect(routeDrop(['C:/a.jar'], { ...base, canInstallMods: false })).toBeNull();
    expect(
      routeDrop(['C:/a.jar'], {
        ...base,
        mode: 'servers',
        serverAddonsKind: 'mod',
        serverCanMutate: false,
      }),
    ).toBeNull();
  });
});
