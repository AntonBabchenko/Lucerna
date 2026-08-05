import { describe, expect, it } from 'vitest';
import type { VtCategory } from '$lib/ipc/bindings';
import {
  conflictsFor,
  installedVtPacks,
  packId,
  toSelection,
} from '$lib/vanillatweaks/vt-selection';

const CATEGORIES: VtCategory[] = [
  {
    category: 'survival',
    packs: [
      {
        name: 'graves',
        display: 'Graves',
        version: '2.8.5',
        description: '',
        incompatible: ['armor statues'],
      },
      {
        name: 'armor statues',
        display: 'Armor Statues',
        version: '2.8.21',
        description: '',
        incompatible: [],
      },
    ],
  },
  {
    category: 'utilities',
    packs: [
      {
        name: 'afk display',
        display: 'AFK Display',
        version: '1.1.2',
        description: '',
        incompatible: [],
      },
    ],
  },
];

describe('toSelection', () => {
  it('groups the ticked packs by category', () => {
    const out = toSelection(CATEGORIES, new Set(['survival/graves', 'utilities/afk display']));
    expect(out).toEqual([
      ['survival', ['graves']],
      ['utilities', ['afk display']],
    ]);
  });

  it('omits a category with nothing ticked', () => {
    const out = toSelection(CATEGORIES, new Set(['utilities/afk display']));
    expect(out).toEqual([['utilities', ['afk display']]]);
  });

  it('is empty when nothing is ticked', () => {
    expect(toSelection(CATEGORIES, new Set())).toEqual([]);
  });
});

describe('conflictsFor', () => {
  it('names a ticked pack the given pack declares incompatible', () => {
    const ticked = new Set(['survival/armor statues']);
    expect(conflictsFor(CATEGORIES[0].packs[0], CATEGORIES, ticked)).toEqual(['Armor Statues']);
  });

  it('reports the conflict in both directions, not only where it is declared', () => {
    // "armor statues" declares nothing; "graves" names it. Ticking graves must
    // still flag armor statues.
    const ticked = new Set(['survival/graves']);
    expect(conflictsFor(CATEGORIES[0].packs[1], CATEGORIES, ticked)).toEqual(['Graves']);
  });

  it('is empty when nothing conflicting is ticked', () => {
    expect(conflictsFor(CATEGORIES[0].packs[0], CATEGORIES, new Set())).toEqual([]);
  });

  it('never reports a pack against itself', () => {
    const ticked = new Set(['survival/graves']);
    expect(conflictsFor(CATEGORIES[0].packs[0], CATEGORIES, ticked)).toEqual([]);
  });
});

describe('installedVtPacks', () => {
  it('maps a Vanilla Tweaks row to its pack id and version', () => {
    const rows = [
      {
        filename: 'graves v2.8.5.zip',
        source: 'vanilla_tweaks' as const,
        project_id: 'survival/graves',
        version_id: '2.8.5',
      },
      {
        filename: 'terralith.zip',
        source: 'modrinth' as const,
        project_id: 'abcdefgh',
        version_id: 'xyz',
      },
    ];
    expect(installedVtPacks(rows)).toEqual(new Map([['survival/graves', '2.8.5']]));
  });

  it('ignores a row with no source at all', () => {
    const rows = [{ filename: 'hand-made.zip', source: null, project_id: null, version_id: null }];
    expect(installedVtPacks(rows)).toEqual(new Map());
  });
});

describe('packId', () => {
  it('joins the category and the pack name', () => {
    expect(packId('survival', CATEGORIES[0].packs[0])).toBe('survival/graves');
  });
});
