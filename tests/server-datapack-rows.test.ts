import { describe, expect, test } from 'vitest';
import type { ServerDatapackEntry } from '$lib/ipc/bindings';
import { badgeOf, isUpdatable, rowKey } from '$lib/servers/datapacks/datapack-rows';

function entry(over: Partial<ServerDatapackEntry> = {}): ServerDatapackEntry {
  return {
    record: {
      filename: 'p.zip',
      sha1: 'a'.repeat(40),
      source: 'modrinth',
      project_id: 'terralith',
      version_id: 'v1',
      name: 'Terralith',
      version_number: '2.5.0',
      enrich_attempted: false,
    },
    state: 'enabled',
    present: true,
    is_folder: false,
    ...over,
  };
}

describe('badgeOf', () => {
  test('maps the two live states to their own badges', () => {
    expect(badgeOf(entry({ state: 'enabled' })).labelKey).toBe('worlds.datapacks.stateEnabled');
    expect(badgeOf(entry({ state: 'disabled' })).labelKey).toBe('worlds.datapacks.stateDisabled');
  });

  test('collapses BOTH ghost states onto one server-scoped badge', () => {
    // The backend distinguishes Orphaned (in the Enabled list) from NotAdded
    // (Disabled only), but to a server admin both mean the same thing: a
    // level.dat entry with no file. The client's own stateNotAdded copy reads
    // "Not in this world", which is meaningless for a server with one world.
    const orphaned = badgeOf(entry({ state: 'orphaned', present: false }));
    const notAdded = badgeOf(entry({ state: 'not_added', present: false }));
    expect(orphaned.labelKey).toBe('servers.datapacks.stateGhost');
    expect(notAdded.labelKey).toBe('servers.datapacks.stateGhost');
    expect(orphaned.variant).toBe(notAdded.variant);
  });

  test('a null state renders as unknown, never as a guess', () => {
    expect(badgeOf(entry({ state: null })).labelKey).toBe('addons.datapacks.stateUnknown');
  });
});

describe('rowKey', () => {
  test('keys on the case-folded filename, not sha1', () => {
    // A folder pack and a ghost row both carry an empty sha1, so a sha1 key
    // would collide every one of them onto ''.
    const folder = entry({
      record: { ...entry().record, filename: 'Folder', sha1: '' },
      is_folder: true,
    });
    const ghost = entry({
      record: { ...entry().record, filename: 'gone.zip', sha1: '' },
      present: false,
    });
    expect(rowKey(folder)).not.toBe(rowKey(ghost));
    expect(rowKey(entry({ record: { ...entry().record, filename: 'P.ZIP' } }))).toBe('p.zip');
  });
});

describe('isUpdatable', () => {
  test('only a present pack with full provenance can be updated', () => {
    expect(isUpdatable(entry())).toBe(true);
    expect(isUpdatable(entry({ is_folder: true }))).toBe(false);
    expect(isUpdatable(entry({ present: false }))).toBe(false);
    expect(
      isUpdatable(entry({ record: { ...entry().record, source: null, project_id: null } })),
    ).toBe(false);
  });
});
