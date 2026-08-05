import { describe, expect, it } from 'vitest';
import type { ModpackVersionEntry } from '$lib/ipc/bindings';
import { joinSummary, summarisePick } from '$lib/modpacks/install-summary';

const modrinth: ModpackVersionEntry = {
  id: 'v1',
  name: 'Pack 1.0',
  version_number: '1.0',
  game_versions: ['1.20.1'],
  loaders: ['fabric'],
  date_published: '2026-01-01T00:00:00Z',
};

describe('summarisePick', () => {
  it('names the pick "newest" when no MC filter is active', () => {
    expect(summarisePick(modrinth, null).reason).toBe('newest');
  });

  it('names the pick "newestFiltered" when an MC filter is active', () => {
    expect(summarisePick(modrinth, '1.20.1').reason).toBe('newestFiltered');
  });

  it('joins multiple game versions and loaders', () => {
    const multi = {
      ...modrinth,
      game_versions: ['1.20.1', '1.20.2'],
      loaders: ['forge', 'neoforge'],
    };
    const pick = summarisePick(multi, null);
    expect(pick.mc).toBe('1.20.1, 1.20.2');
    expect(pick.loaders).toBe('Forge, NeoForge');
  });

  // ATLauncher version entries, and CurseForge ones whose file names no
  // loader, report an empty `loaders`. That must read as "unknown", not "".
  it('reports a null loader when the source names none', () => {
    expect(summarisePick({ ...modrinth, loaders: [] }, null).loaders).toBeNull();
  });

  it('reports a null mc when the source names none', () => {
    expect(summarisePick({ ...modrinth, game_versions: [] }, null).mc).toBeNull();
  });
});

describe('joinSummary', () => {
  it('joins present segments with a middot', () => {
    expect(joinSummary(['Newest version', 'Minecraft 1.20.1', 'Fabric'])).toBe(
      'Newest version · Minecraft 1.20.1 · Fabric',
    );
  });

  // The whole point: a CurseForge pack with no loader must not render
  // "Newest version · Minecraft 1.20.1 · ".
  it('drops null and blank segments without leaving a separator', () => {
    expect(joinSummary(['Newest version', 'Minecraft 1.20.1', null])).toBe(
      'Newest version · Minecraft 1.20.1',
    );
    expect(joinSummary([null, '  ', 'Fabric'])).toBe('Fabric');
  });
});
