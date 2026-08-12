import { beforeEach, describe, expect, test } from 'vitest';
import { markSeen } from '$lib/onboarding/contextual-tours';
import { kindsFor } from '$lib/servers/addons/addon-kinds';

// The Add-ons tab hosts the `serverAddons` contextual tour. Nothing here
// renders it today (these are pure `kindsFor` assertions), but the seed keeps
// the file safe to grow into a rendering test without a tour popover suddenly
// sitting on top of the assertions.
beforeEach(() => markSeen('serverAddons'));

describe('kindsFor', () => {
  test('a 1.21 fabric server offers mods and datapacks', () => {
    expect(kindsFor('fabric', true)).toEqual(['mod', 'datapack']);
  });

  test('a 1.21 paper server offers plugins and datapacks', () => {
    expect(kindsFor('paper', true)).toEqual(['plugin', 'datapack']);
  });

  test('a pre-1.13 server is not offered the datapack kind', () => {
    // The whole feature is inert there: no world datapacks dir, and level.dat
    // Enabled/Disabled tags a pre-1.13 server never reads — the launcher would
    // render states that are pure fiction.
    expect(kindsFor('fabric', false)).toEqual(['mod']);
  });

  test('a pre-1.13 VANILLA server offers no kinds at all', () => {
    // The hole the gate opens: vanilla is neither mod- nor plugin-capable, so
    // dropping datapack empties the list. `kinds[0]` would then be undefined —
    // zero tabs, a live dropzone with a datapack filter, and the pane block's
    // else-arm mounting the plugin browser for a vanilla server.
    expect(kindsFor('vanilla', false)).toEqual([]);
  });

  test('a 1.21 vanilla server offers datapacks only', () => {
    expect(kindsFor('vanilla', true)).toEqual(['datapack']);
  });
});
