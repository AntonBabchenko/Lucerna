// tests/installed/installed-datapacks-bulk.test.ts
// The library screen's bulk bar: Update over the selected packs with a pending update (the kept-
// old-copy outcome names its worlds and counts as failed), Remove through the batch dialog with
// the one cascade answer, and the running gate.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModVersion } from '$lib/ipc/bindings';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  datapacksCheckUpdates: vi.fn(),
  datapacksUpdateOne: vi.fn(),
  datapacksRemoveFromLibrary: vi.fn(),
  datapacksRemoveFromWorld: vi.fn(),
  datapacksWorldEntryKind: vi.fn(),
  datapacksSetEnabledInWorld: vi.fn(),
  runningInstances: vi.fn(),
  modsProjects: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
const toasts = vi.hoisted(() => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushInfo: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';

const V2: ModVersion = {
  source: 'modrinth',
  project_id: 'vm',
  version_id: 'v2',
  name: 'VeinMiner',
  version_number: '2.0',
  mc_versions: ['1.21.1'],
  loaders: [],
  primary_file: {
    filename: 'vm-2.zip',
    url: 'https://x/vm-2.zip',
    sha1: 'b',
    size: 1,
    distribution_allowed: true,
  },
  deps: [],
  published_at: null,
};
const pack = (filename: string, name: string, catalog = true) => ({
  filename,
  sha1: 'a'.repeat(40),
  size_bytes: 1024,
  name,
  source: catalog ? ('modrinth' as const) : null,
  project_id: catalog ? 'vm' : null,
  version_id: catalog ? 'v1' : null,
  version_number: catalog ? '1.0' : null,
  installed_at: '2026-09-24T00:00:00Z',
});
const entry = (filename: string, name: string, over: Record<string, unknown> = {}) => ({
  pack: pack(filename, name),
  in_library: true,
  compat: { kind: 'compatible' },
  placements: [{ world: 'Alpha', state: 'enabled', ignored_reason: null, level_dat: 'present' }],
  ...over,
});
function library(entries: unknown[]) {
  cmd.datapacksListLibrary.mockResolvedValue({
    status: 'ok',
    data: { entries, worlds: [{ world: 'Alpha', level_dat: 'present' }] },
  });
}
const mount = () =>
  render(InstalledDatapacksView, { props: { instanceId: 'inst-1', mcVersion: '1.21.1' } });

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  cmd.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
  cmd.datapacksRemoveFromLibrary.mockResolvedValue({
    status: 'ok',
    data: { worlds: [{ kind: 'removed', world: 'Alpha' }], removed_from_library: true },
  });
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => vi.clearAllMocks());

describe('InstalledDatapacksView — bulk actions', () => {
  it('Remove opens the batch dialog for the selection and removes each with the cascade answer', async () => {
    library([entry('a.zip', 'Alpha Pack'), entry('b.zip', 'Beta Pack')]);
    mount();
    await fireEvent.click(await screen.findByTestId('bulk-select-all'));
    expect(screen.getByText(/2 selected/)).toBeTruthy();
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    expect(await screen.findByTestId('datapack-bulk-remove-dialog')).toBeTruthy();
    expect(cmd.datapacksRemoveFromLibrary).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByTestId('datapack-bulk-remove-confirm'));
    await waitFor(() => expect(cmd.datapacksRemoveFromLibrary).toHaveBeenCalledTimes(2));
    expect(cmd.datapacksRemoveFromLibrary).toHaveBeenCalledWith('inst-1', 'a.zip', true);
    // The owner re-lists after the run.
    await waitFor(() =>
      expect(cmd.datapacksListLibrary.mock.calls.length).toBeGreaterThanOrEqual(2),
    );
  });

  it('Update applies to the selected packs with a pending update; a kept old copy counts as failed and names its worlds', async () => {
    library([entry('a.zip', 'Alpha Pack'), entry('b.zip', 'Beta Pack')]);
    cmd.datapacksCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [
        { filename: 'a.zip', name: 'Alpha Pack', state: { kind: 'update_available', latest: V2 } },
      ],
    });
    cmd.datapacksUpdateOne.mockResolvedValue({
      status: 'ok',
      data: {
        pack: pack('vm-2.zip', 'Alpha Pack'),
        migrations: [
          {
            kind: 'failed',
            world: 'Alpha',
            error: { kind: 'io', path: 'saves/Alpha', details: 'locked' },
          },
        ],
        completed: false,
        old_copy_kept: true,
      },
    });
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: /check for updates/i }));
    await fireEvent.click(await screen.findByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    await waitFor(() =>
      expect(within(bar).getByRole('button', { name: 'Update' }).hasAttribute('disabled')).toBe(
        false,
      ),
    );
    await fireEvent.click(within(bar).getByRole('button', { name: 'Update' }));
    await waitFor(() => expect(cmd.datapacksUpdateOne).toHaveBeenCalledTimes(1));
    expect(cmd.datapacksUpdateOne).toHaveBeenCalledWith('inst-1', 'a.zip', V2);
    // The worlds left behind, by name (the single row's warning) …
    await waitFor(() => expect(toasts.pushWarning).toHaveBeenCalledTimes(2));
    const titles = toasts.pushWarning.mock.calls.map((c) => c[0]);
    expect(titles.some((x: string) => /retry can finish/.test(x))).toBe(true);
    // … and the pack as failed in the run's notice, under the reason.
    const run = toasts.pushWarning.mock.calls.find((c) => c[0] === 'Updated 0 of 1, 1 failed');
    expect(run?.[1]).toEqual([
      { names: 'Alpha Pack', reason: 'The old version was kept so a retry can finish' },
    ]);
    // The badge stays for the retry.
    expect(screen.getByTestId('datapack-update-btn')).toBeTruthy();
  });

  it('while the profile runs every bar action is off with the gate note’s reason', async () => {
    cmd.runningInstances.mockResolvedValue([{ instance_id: 'inst-1', pid: 1 }]);
    library([entry('a.zip', 'Alpha Pack')]);
    mount();
    await fireEvent.click(await screen.findByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    for (const name of ['Update', 'Remove']) {
      const btn = within(bar).getByRole('button', { name });
      expect(btn.hasAttribute('disabled')).toBe(true);
      expect(btn.parentElement?.getAttribute('tabindex')).toBe('0');
    }
  });
});
