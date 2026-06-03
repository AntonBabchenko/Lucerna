import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  modsPackOriginSummary: vi.fn(),
  modsProject: vi.fn(),
  modsEnrichPackMods: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import { createInstalledData } from '$lib/mods/installed/installed-data.svelte';

const row = (sha1: string, source: string | null) => ({
  filename: `${sha1}.jar`,
  sha1,
  source,
  project_id: source ? 'p' : null,
  version_id: source ? 'v' : null,
  name: sha1.toUpperCase(),
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
});

describe('createInstalledData', () => {
  it('loads installed rows and resolves summaries', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [row('a', 'modrinth')] });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProject.mockResolvedValue({
      status: 'ok',
      data: { summary: { source: 'modrinth', project_id: 'p', slug: 's', name: 'Alpha', summary: '', icon_url: null, downloads: 1, author: 'x', updated_at: null }, description: '', website_url: null },
    });
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.rows).toHaveLength(1);
    expect(data.rows[0].summary?.name).toBe('Alpha');
    expect(data.loading).toBe(false);
  });

  it('keeps manual mods (source: null) as degraded rows without a project lookup', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [row('m', null)] });
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProject.mockClear();
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.rows[0].summary).toBeNull();
    expect(mocks.modsProject).not.toHaveBeenCalled();
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
    mocks.modsListInstalled.mockReset();
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
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
