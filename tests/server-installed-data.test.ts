import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  serverListModsEnriched: vi.fn(),
  serverListPluginsEnriched: vi.fn(),
  serverEnrichMods: vi.fn(),
  serverEnrichPlugins: vi.fn(),
  modsProjects: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import {
  createServerInstalledData,
  groupProjectIdsBySource,
} from '$lib/servers/addons/server-installed-data.svelte';

// A ServerModEntryEnriched-shaped row. `source: null` => loose (no identity),
// which mirrors an override-bundled jar the backend could not fingerprint.
const modRow = (
  sha1: string,
  source: string | null,
  opts: { projectId?: string; disabled?: boolean; reason?: string | null } = {},
) => ({
  filename: `${sha1}.jar`,
  on_disk_filename: opts.disabled ? `${sha1}.jar.disabled` : `${sha1}.jar`,
  disabled: opts.disabled ?? false,
  reason: opts.reason ?? null,
  sha1,
  source,
  project_id: source ? (opts.projectId ?? 'p') : null,
  version_id: source ? 'v' : null,
  name: source ? sha1.toUpperCase() : null,
  version_number: source ? '1.0' : null,
});

const summary = (source: string, projectId: string, name: string) => ({
  source,
  project_id: projectId,
  slug: 's',
  name,
  summary: '',
  icon_url: null,
  downloads: 1,
  author: 'x',
  updated_at: null,
});

describe('groupProjectIdsBySource', () => {
  it('groups project_ids by source, skipping identity-less rows', () => {
    const map = groupProjectIdsBySource([
      { source: 'modrinth', project_id: 'a' },
      { source: 'modrinth', project_id: 'b' },
      { source: 'curseforge', project_id: 'c' },
      { source: null, project_id: null },
    ]);
    expect([...(map.get('modrinth') ?? [])].sort()).toEqual(['a', 'b']);
    expect([...(map.get('curseforge') ?? [])]).toEqual(['c']);
  });
});

describe('createServerInstalledData', () => {
  beforeEach(() => vi.clearAllMocks());

  it('maps enriched rows to cards, attaching summaries only where identity resolves', async () => {
    mocks.serverListModsEnriched.mockResolvedValue({
      status: 'ok',
      data: [
        modRow('a', 'modrinth', { projectId: 'p', disabled: true, reason: 'client_only' }),
        modRow('b', null),
      ],
    });
    // A loose row is present, so the one-shot backfill fires; returning 0 keeps
    // the list unchanged (re-list guarded separately below).
    mocks.serverEnrichMods.mockResolvedValue({ status: 'ok', data: 0 });
    mocks.modsProjects.mockResolvedValue({
      status: 'ok',
      data: [summary('modrinth', 'p', 'Alpha')],
    });

    const data = createServerInstalledData(
      () => 's1',
      'mod',
      () => 0,
    );
    await data.refresh();

    expect(data.rows).toHaveLength(2);
    const identified = data.rows.find((r) => r.sha1 === 'a');
    expect(identified?.card.summary?.name).toBe('Alpha');
    expect(identified?.reason).toBe('client_only');
    expect(identified?.onDiskFilename).toBe('a.jar.disabled');
    expect(identified?.card.installed.enabled).toBe(false);

    const loose = data.rows.find((r) => r.sha1 === 'b');
    expect(loose?.card.summary).toBeNull();
    expect(loose?.reason).toBeNull();
    expect(loose?.onDiskFilename).toBe('b.jar');

    // Identity is resolved in a single batched lookup, not per row.
    expect(mocks.modsProjects).toHaveBeenCalledWith('modrinth', ['p']);
    expect(data.loading).toBe(false);
  });

  it('runs the enrich backfill and re-lists when a row lacks identity', async () => {
    mocks.serverListModsEnriched
      .mockResolvedValueOnce({ status: 'ok', data: [modRow('x', null)] })
      .mockResolvedValueOnce({
        status: 'ok',
        data: [modRow('x', 'modrinth', { projectId: 'p' })],
      });
    // The pass identified 1 jar, so the list is re-fetched.
    mocks.serverEnrichMods.mockResolvedValue({ status: 'ok', data: 1 });
    mocks.modsProjects.mockResolvedValue({ status: 'ok', data: [summary('modrinth', 'p', 'X')] });

    const data = createServerInstalledData(
      () => 's1',
      'mod',
      () => 0,
    );
    await data.refresh();

    expect(mocks.serverEnrichMods).toHaveBeenCalledTimes(1);
    expect(mocks.serverListModsEnriched).toHaveBeenCalledTimes(2);
    // The re-fetched (now identified) row is what surfaces.
    expect(data.rows).toHaveLength(1);
    expect(data.rows[0].card.summary?.name).toBe('X');
  });

  it('skips the enrich backfill when every row already has identity', async () => {
    mocks.serverListModsEnriched.mockResolvedValue({
      status: 'ok',
      data: [modRow('a', 'modrinth', { projectId: 'p' })],
    });
    mocks.modsProjects.mockResolvedValue({
      status: 'ok',
      data: [summary('modrinth', 'p', 'Alpha')],
    });

    const data = createServerInstalledData(
      () => 's1',
      'mod',
      () => 0,
    );
    await data.refresh();

    expect(mocks.serverEnrichMods).not.toHaveBeenCalled();
    expect(mocks.serverListModsEnriched).toHaveBeenCalledTimes(1);
  });

  it('does not re-list when the enrich pass identifies nothing', async () => {
    mocks.serverListModsEnriched.mockResolvedValue({ status: 'ok', data: [modRow('x', null)] });
    mocks.serverEnrichMods.mockResolvedValue({ status: 'ok', data: 0 });

    const data = createServerInstalledData(
      () => 's1',
      'mod',
      () => 0,
    );
    await data.refresh();

    expect(mocks.serverEnrichMods).toHaveBeenCalledTimes(1);
    // data === 0 ⇒ the second list call is skipped.
    expect(mocks.serverListModsEnriched).toHaveBeenCalledTimes(1);
  });

  it('drops a stale refresh once a newer one supersedes it (server switch)', async () => {
    // The list for server "A" hangs until we release it. Meanwhile the user
    // switches to "B" — which, in the app, re-runs the $effect and fires a fresh
    // refresh. That newer refresh bumps the monotonic token, so the resumed "A"
    // refresh must NOT commit its now-stale rows over "B"'s result.
    let releaseA: (v: unknown) => void = () => {};
    const aPending = new Promise((res) => (releaseA = res));
    mocks.serverEnrichMods.mockResolvedValue({ status: 'ok', data: 0 });
    mocks.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
    mocks.serverListModsEnriched.mockImplementation((id: string) =>
      id === 'A'
        ? aPending.then(() => ({
            status: 'ok',
            data: [modRow('stale', 'modrinth', { projectId: 'p' })],
          }))
        : Promise.resolve({ status: 'ok', data: [] }),
    );

    let current = 'A';
    const data = createServerInstalledData(
      () => current,
      'mod',
      () => 0,
    );
    const inflightA = data.refresh(); // token 1, for "A" (hangs)
    current = 'B'; // user switches servers while "A" is still loading
    const inflightB = data.refresh(); // token 2, for "B" (resolves empty)
    await inflightB;
    releaseA(null); // "A"'s list now resolves — but the token has moved on
    await inflightA;

    // The stale "A" rows must NOT have overwritten "B"'s committed (empty) list.
    expect(data.rows).toHaveLength(0);
  });
});
