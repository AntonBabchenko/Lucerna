import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  modsPackOriginSummary: vi.fn(),
  modsProjects: vi.fn(),
  modsEnrichPackMods: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import { createInstalledData } from '$lib/mods/installed/installed-data.svelte';

const row = (sha1: string, source: string | null, projectId = 'p') => ({
  filename: `${sha1}.jar`,
  sha1,
  source,
  project_id: source ? projectId : null,
  version_id: source ? 'v' : null,
  name: sha1.toUpperCase(),
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
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

describe('createInstalledData', () => {
  beforeEach(() => vi.clearAllMocks());

  it('loads installed rows and resolves summaries via one batched lookup', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [row('a', 'modrinth')] });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProjects.mockResolvedValue({
      status: 'ok',
      data: [summary('modrinth', 'p', 'Alpha')],
    });
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.rows).toHaveLength(1);
    expect(data.rows[0].summary?.name).toBe('Alpha');
    expect(data.loading).toBe(false);
    // The source is resolved with a single batched id list, not per mod.
    expect(mocks.modsProjects).toHaveBeenCalledWith('modrinth', ['p']);
  });

  it('batches one request per source', async () => {
    mocks.modsListInstalled.mockResolvedValue({
      status: 'ok',
      data: [row('a', 'modrinth', 'm1'), row('b', 'curseforge', '42'), row('c', 'modrinth', 'm2')],
    });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProjects.mockImplementation((source: string, ids: string[]) =>
      Promise.resolve({ status: 'ok', data: ids.map((id) => summary(source, id, `N-${id}`)) }),
    );
    const data = createInstalledData(() => 'i');
    await data.refresh();
    // Each source is fetched once with ALL its ids batched into a single call —
    // the two modrinth mods come back in one ['m1','m2'] request, not two.
    expect(mocks.modsProjects).toHaveBeenCalledWith('modrinth', ['m1', 'm2']);
    expect(mocks.modsProjects).toHaveBeenCalledWith('curseforge', ['42']);
    expect(data.rows).toHaveLength(3);
    expect(data.rows.find((r) => r.installed.sha1 === 'b')?.summary?.name).toBe('N-42');
    expect(data.rows.find((r) => r.installed.sha1 === 'a')?.summary?.name).toBe('N-m1');
  });

  it('keeps manual mods (source: null) as degraded rows without a lookup', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [row('m', null)] });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.rows[0].summary).toBeNull();
    expect(mocks.modsProjects).not.toHaveBeenCalled();
  });

  it('degrades rows when the batch lookup fails, without a banner', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [row('a', 'modrinth')] });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProjects.mockResolvedValue({ status: 'error', error: 'boom' });
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.rows).toHaveLength(1);
    expect(data.rows[0].summary).toBeNull();
    // A metadata-fetch failure is cosmetic — it must NOT surface an error banner.
    expect(data.error).toBeNull();
  });

  it('surfaces a list error', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'error', error: 'boom' });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.error).toBe('boom');
    expect(data.loading).toBe(false);
  });

  it('drops a stale refresh when the instance changes mid-flight', async () => {
    // modsListInstalled for instance "A" hangs until we signal it.
    let releaseA: (v: unknown) => void = () => {};
    const aPending = new Promise((res) => (releaseA = res));
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
    mocks.modsListInstalled.mockImplementation((id: string) =>
      id === 'A'
        ? aPending.then(() => ({ status: 'ok', data: [row('stale', 'modrinth')] }))
        : Promise.resolve({ status: 'ok', data: [] }),
    );

    let current = 'A';
    const data = createInstalledData(() => current);
    const inflight = data.refresh(); // started for "A"
    current = 'B'; // user switches instances while "A" is still loading
    releaseA(null); // now "A"'s list resolves — but it is stale
    await inflight;

    // The stale "A" result must NOT have been committed under instance "B".
    expect(data.rows).toHaveLength(0);
  });
});
