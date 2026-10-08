/**
 * Spec 2026-10-08 aliases-everywhere D3/D4: the frontend side of the cross-source ids is one
 * module — the learning pass (one per profile at a time, once more if asked meanwhile), the
 * per-profile generation the page moves, the alias map, and the one lookup every «installed?»
 * question about a project asks.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledMod } from '$lib/ipc/bindings';
import {
  aliasesFor,
  bumpCrossIds,
  crossIdsGeneration,
  findInstalled,
  learnCrossIds,
  loadAliases,
  resetCrossIdsForTests,
} from '$lib/mods/cross-ids.svelte';

// Filled per test in `beforeEach`; a test may delete a command to play an older mock.
const h = vi.hoisted(() => ({ commands: {} as Record<string, unknown> }));
vi.mock('$lib/ipc/bindings', () => ({ commands: h.commands }));

const learn = () => h.commands.modsLearnCrossIds as ReturnType<typeof vi.fn>;

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const mod = (source: 'modrinth' | 'curseforge', projectId: string, name: string) => ({
  installed: {
    filename: `${name}.jar`,
    sha1: `${projectId}-sha`,
    source,
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  } satisfies InstalledMod,
});

beforeEach(() => {
  resetCrossIdsForTests();
  h.commands.modsLearnCrossIds = vi.fn();
  h.commands.modsCrossAliases = vi.fn();
  vi.spyOn(console, 'warn').mockImplementation(() => {});
});

describe('learnCrossIds', () => {
  it('runs one pass per profile at a time, and once more when asked meanwhile', async () => {
    const first = deferred<unknown>();
    learn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue({ status: 'ok', data: { learned: 0 } });
    const a = learnCrossIds('p');
    const b = learnCrossIds('p'); // joins the running pass
    const c = learnCrossIds('p');
    expect(learn()).toHaveBeenCalledTimes(1);
    first.resolve({ status: 'ok', data: { learned: 0 } });
    await Promise.all([a, b, c]);
    expect(learn()).toHaveBeenCalledTimes(2); // the one rerun, not one per call
    await learnCrossIds('p');
    expect(learn()).toHaveBeenCalledTimes(3);
  });

  it('another profile has its own pass', async () => {
    learn().mockResolvedValue({ status: 'ok', data: { learned: 0 } });
    await Promise.all([learnCrossIds('p'), learnCrossIds('q')]);
    expect(
      learn()
        .mock.calls.map((c) => c[0])
        .sort(),
    ).toEqual(['p', 'q']);
  });

  it('a pass that throws is logged and resolves', async () => {
    learn().mockRejectedValue(new Error('ipc down'));
    await expect(learnCrossIds('p')).resolves.toBeUndefined();
    expect(console.warn).toHaveBeenCalled();
  });
});

describe('the generation', () => {
  it('starts at 0 and moves per profile', () => {
    expect(crossIdsGeneration('p')).toBe(0);
    bumpCrossIds('p');
    bumpCrossIds('p');
    expect(crossIdsGeneration('p')).toBe(2);
    expect(crossIdsGeneration('q')).toBe(0);
  });
});

describe('loadAliases', () => {
  it('keys the map alias → own', async () => {
    (h.commands.modsCrossAliases as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [
        {
          alias_source: 'curseforge',
          alias_project_id: '258587',
          own_source: 'modrinth',
          own_project_id: 'MJX',
        },
      ],
    });
    await loadAliases('p');
    expect(aliasesFor('p').get('curseforge:258587')).toBe('modrinth:MJX');
  });

  it('an older mock without the command leaves the map empty and throws nothing', async () => {
    h.commands.modsCrossAliases = undefined;
    await expect(loadAliases('p')).resolves.toBeUndefined();
    expect(aliasesFor('p').size).toBe(0);
  });

  it('an error answer keeps the map it had', async () => {
    const cmd = h.commands.modsCrossAliases as ReturnType<typeof vi.fn>;
    cmd.mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          alias_source: 'curseforge',
          alias_project_id: '1',
          own_source: 'modrinth',
          own_project_id: 'A',
        },
      ],
    });
    await loadAliases('p');
    cmd.mockResolvedValueOnce({ status: 'error', error: { kind: 'io' } });
    await loadAliases('p');
    expect(aliasesFor('p').get('curseforge:1')).toBe('modrinth:A');
  });
});

describe('findInstalled', () => {
  const rows = [mod('modrinth', 'MJX', 'Parasites'), mod('modrinth', 'CLOTH', 'Cloth Config')];
  const aliases = new Map([['curseforge:258587', 'modrinth:MJX']]);

  it('finds a row by its own key', () => {
    expect(findInstalled(rows, aliases, 'modrinth', 'MJX')).toBe(rows[0]);
  });

  it('finds the row a project is an alias of', () => {
    expect(findInstalled(rows, aliases, 'curseforge', '258587')).toBe(rows[0]);
  });

  it('never matches by name', () => {
    // CurseForge's «Cloth Config API» has no alias here: whatever its name, it is not that row.
    expect(findInstalled(rows, aliases, 'curseforge', '348521')).toBeNull();
  });
});
