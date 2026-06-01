import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledModsView from '$lib/mods/InstalledModsView.svelte';

vi.mock('$lib/ipc/bindings', async (orig) => {
  const actual = await orig<typeof import('$lib/ipc/bindings')>();
  const mod = (sha1: string, name: string, enabled = true) => ({
    filename: `${name}.jar`, sha1, source: 'modrinth', project_id: name, version_id: 'v',
    name, version_number: '1.0', installed_at: '2026-01-01T00:00:00Z', enabled,
    enrich_attempted: false, requires: [],
  });
  return {
    ...actual,
    commands: {
      ...actual.commands,
      modsListInstalled: vi.fn(async () => ({ status: 'ok', data: [mod('a', 'Alpha'), mod('b', 'Beta', false)] })),
      modsPackOriginSummary: vi.fn(async () => ({ status: 'ok', data: null })),
      modsProject: vi.fn(async (_s: unknown, id: string) => ({ status: 'ok', data: { summary: { source: 'modrinth', project_id: id, slug: id, name: id, summary: '', icon_url: null, downloads: 0, author: 'x', updated_at: null } } })),
      modsDependencyGraph: vi.fn(async () => ({ status: 'ok', data: { roots: [] } })),
    },
    events: Object.fromEntries(Object.keys(actual.events).map((k) => [k, { listen: async () => () => {} }])),
  };
});

const props = { instanceId: 'inst1', mcVersion: '1.20.1', loader: 'fabric' as const };

describe('InstalledModsView selection', () => {
  it('Select all checks every filtered row', async () => {
    render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    const selectAll = await screen.findByLabelText(/select all/i);
    await fireEvent.click(selectAll);
    const boxes = screen.getAllByRole('checkbox', { name: /select mod/i });
    expect(boxes.every((b) => (b as HTMLInputElement).checked)).toBe(true);
  });

  it('shows the bulk bar with a count when a row is selected', async () => {
    render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    const first = screen.getAllByRole('checkbox', { name: /select mod/i })[0];
    await fireEvent.click(first);
    expect(screen.getByText(/1 selected/i)).toBeTruthy();
  });
});
