import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsFindOrphans: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn(), pushWarning: vi.fn() }));
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({ get: () => (k: string) => k }));

import { createInstalledSelection } from '$lib/mods/installed/installed-selection.svelte';
import type { Row } from '$lib/mods/installed/installed-data.svelte';

const row = (sha1: string, enabled: boolean): Row => ({
  summary: null,
  installed: {
    filename: `${sha1}.jar`,
    sha1,
    source: 'modrinth',
    project_id: 'p' + sha1,
    version_id: 'v',
    name: sha1.toUpperCase(),
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled,
    enrich_attempted: false,
  },
});

const noop = async () => {};

describe('createInstalledSelection', () => {
  it('toggles a row and computes allSelected', () => {
    const rows = [row('a', true), row('b', true)];
    const s = createInstalledSelection(
      () => rows,
      () => 'i',
      noop,
      () => new Map(),
      () => {},
    );
    s.toggleSelect('a', true);
    expect(s.selected.has('a')).toBe(true);
    expect(s.allSelected).toBe(false);
    s.toggleSelectAll(true);
    expect(s.allSelected).toBe(true);
  });

  it('bulkSetEnabled calls the right IPC for rows that need flipping', async () => {
    const rows = [row('a', false), row('b', true)];
    const s = createInstalledSelection(
      () => rows,
      () => 'i',
      noop,
      () => new Map(),
      () => {},
    );
    s.toggleSelectAll(true);
    await s.bulkSetEnabled(true);
    expect(mocks.modsEnable).toHaveBeenCalledWith('i', 'a');
    expect(mocks.modsEnable).not.toHaveBeenCalledWith('i', 'b'); // already enabled
  });

  it('requestBulkUninstall populates the orphan prompt', async () => {
    const rows = [row('a', true)];
    const s = createInstalledSelection(
      () => rows,
      () => 'i',
      noop,
      () => new Map(),
      () => {},
    );
    s.toggleSelectAll(true);
    await s.requestBulkUninstall();
    expect(s.uninstallPrompt?.removing).toEqual(['a']);
  });
});
