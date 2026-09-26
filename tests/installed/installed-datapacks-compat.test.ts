// The library row's compatibility warning (§1 C5, §0.5 A12): one icon on
// the collapsed row whose tooltip and accessible name are the world tab's
// own sentence — the game's verdict, never "may stop the world from loading".
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DatapackLibraryEntry, PackCompat } from '$lib/ipc/bindings';

const { datapacksListLibrary, runningInstances, spawnListen, exitListen } = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  spawnListen: vi.fn(),
  exitListen: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { datapacksListLibrary, runningInstances },
  events: {
    processSpawned: { listen: spawnListen },
    processExited: { listen: exitListen },
  },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';

function entry(compat: PackCompat): DatapackLibraryEntry {
  return {
    pack: {
      filename: 'dagger.zip',
      sha1: 'a'.repeat(40),
      size_bytes: 1024,
      name: 'Dagger fix',
      source: null,
      project_id: null,
      version_id: null,
      version_number: null,
      installed_at: '2026-09-24T00:00:00Z',
    },
    in_library: true,
    compat,
    placements: [],
  };
}

function libraryWith(compat: PackCompat) {
  datapacksListLibrary.mockResolvedValue({
    status: 'ok',
    data: { entries: [entry(compat)], worlds: [] },
  });
}

beforeEach(() => {
  runningInstances.mockResolvedValue([]);
  spawnListen.mockResolvedValue(() => {});
  exitListen.mockResolvedValue(() => {});
});
afterEach(() => vi.clearAllMocks());

async function warning(): Promise<HTMLElement> {
  render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
  return screen.findByTestId('datapack-compat-warning');
}

describe('InstalledDatapacksView — compatibility warning', () => {
  it('a too_old entry shows the warning icon whose tooltip is the same line', async () => {
    libraryWith({ kind: 'too_old', made_for: '15', game: '48' });
    const icon = await warning();
    expect(icon.getAttribute('aria-label')).toMatch(/older version of Minecraft.*still loads/);
    expect(icon.getAttribute('aria-label')).not.toMatch(/stop the world from loading/);
    const row = icon.closest('[data-card-shell]');
    expect(row?.querySelector('[data-card-accent]')?.className).toContain('bg-warning-text');
    // Tooltip text is not in the DOM before hover; pin that both read the
    // same value at the source.
    const src = readFileSync(
      resolve(process.cwd(), 'src/lib/mods/InstalledDatapacksView.svelte'),
      'utf8',
    );
    expect(src).toContain('use:tooltip={compatText}');
    expect(src).toContain('aria-label={compatText}');
  });

  it('a wont_load entry gets the same accent and icon (§0.5 A12)', async () => {
    libraryWith({ kind: 'wont_load', reason: 'no_pack_format' });
    const icon = await warning();
    expect(icon.getAttribute('aria-label')).toMatch(/skips this pack/);
    expect(
      icon.closest('[data-card-shell]')?.querySelector('[data-card-accent]')?.className,
    ).toContain('bg-warning-text');
  });

  it('compatible and unknown entries show no warning icon', async () => {
    for (const compat of [{ kind: 'compatible' }, { kind: 'unknown' }] as PackCompat[]) {
      libraryWith(compat);
      const { unmount } = render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
      await screen.findByText('Dagger fix');
      expect(screen.queryByTestId('datapack-compat-warning')).toBeNull();
      unmount();
    }
  });
});
