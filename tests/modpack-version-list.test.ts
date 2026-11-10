import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ModpackVersionEntry } from '$lib/ipc/bindings';
import ModpackVersionList from '$lib/modpacks/ModpackVersionList.svelte';

function ver(id: string, date: string, gameVersions: string[] = ['1.20.1']): ModpackVersionEntry {
  return {
    id,
    name: `Release ${id}`,
    version_number: id,
    game_versions: gameVersions,
    loaders: ['fabric'],
    date_published: date,
  };
}

// Deliberately unsorted input so the component cannot rely on caller ordering.
const VERSIONS = [
  ver('v1', '2026-01-01T00:00:00Z', ['1.19.2']),
  ver('v3', '2026-03-01T00:00:00Z'),
  ver('v2', '2026-02-01T00:00:00Z'),
];

// The pack version filter is the shared Select, not a native <select>. Driving
// one means opening it (click the trigger by data-testid) then committing an
// option — Select commits on `mousedown`.
async function pickOption(testid: string, name: RegExp | string) {
  await fireEvent.click(screen.getByTestId(testid));
  await fireEvent.mouseDown(screen.getByRole('option', { name }));
}

describe('ModpackVersionList', () => {
  it('lists versions newest first regardless of input order', () => {
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
    });
    const rows = screen.getAllByTestId(/^version-row-/);
    expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual([
      'version-row-v3',
      'version-row-v2',
      'version-row-v1',
    ]);
  });

  it('starts filtered to the requested Minecraft version when the pack targets it', () => {
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
      initialMcFilter: '1.19.2',
    });
    const rows = screen.getAllByTestId(/^version-row-/);
    expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual(['version-row-v1']);
  });

  it('ignores a requested Minecraft version the pack never targeted', () => {
    // Filtering to it would show an empty list with no way to tell why.
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
      initialMcFilter: '26.1.1',
    });
    expect(screen.getAllByTestId(/^version-row-/)).toHaveLength(3);
  });

  it('starts filtered to the requested loader too, when the pack spans several', () => {
    const mixed = [
      { ...ver('fab', '2026-03-01T00:00:00Z'), loaders: ['fabric'] },
      { ...ver('neo', '2026-02-01T00:00:00Z'), loaders: ['NeoForge'] },
      { ...ver('any', '2026-01-01T00:00:00Z'), loaders: [] },
    ];
    render(ModpackVersionList, {
      versions: mixed,
      installedVersionId: null,
      onSelect: vi.fn(),
      initialLoaderFilter: 'neoforge',
    });
    expect(screen.getByTestId('version-loader-filter')).toBeTruthy();
    // A version listing no loader cannot be ruled out.
    expect(
      screen.getAllByTestId(/^version-row-/).map((r) => r.getAttribute('data-testid')),
    ).toEqual(['version-row-neo', 'version-row-any']);
  });

  it('shows no loader filter for a single-loader pack', () => {
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
      initialLoaderFilter: 'fabric',
    });
    expect(screen.queryByTestId('version-loader-filter')).toBeNull();
    expect(screen.getAllByTestId(/^version-row-/)).toHaveLength(3);
  });

  it('marks the installed version', () => {
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
    });
    expect(screen.getByTestId('version-row-v2').textContent).toContain('Installed');
  });

  it('marks versions published after the installed one as newer', () => {
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
    });
    expect(screen.getByTestId('version-row-v3').textContent).toContain('Newer');
    expect(screen.getByTestId('version-row-v1').textContent).toContain('Older');
  });

  it('reports the chosen version to the caller', async () => {
    const onSelect = vi.fn();
    render(ModpackVersionList, { versions: VERSIONS, installedVersionId: 'v2', onSelect });
    await fireEvent.click(screen.getByTestId('version-row-v1'));
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'v1' }));
  });

  it('filters by Minecraft version', async () => {
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'v2',
      onSelect: vi.fn(),
    });
    await pickOption('version-mc-filter', '1.19.2');
    const rows = screen.getAllByTestId(/^version-row-/);
    expect(rows).toHaveLength(1);
    expect(rows[0].getAttribute('data-testid')).toBe('version-row-v1');
  });

  it('renders an empty state when the pack has no versions', () => {
    render(ModpackVersionList, { versions: [], installedVersionId: null, onSelect: vi.fn() });
    expect(screen.getByTestId('version-list-empty')).toBeTruthy();
  });

  it('shows no newer/older claim when the installed version is not in the list', () => {
    // Delisted version — placing it would be a guess, so no relation badge.
    render(ModpackVersionList, {
      versions: VERSIONS,
      installedVersionId: 'delisted',
      onSelect: vi.fn(),
    });
    expect(screen.getAllByTestId(/^version-row-/)).toHaveLength(3);
    expect(screen.getByTestId('version-row-v3').textContent).not.toContain('Newer');
    expect(screen.getByTestId('version-row-v1').textContent).not.toContain('Older');
  });
});
