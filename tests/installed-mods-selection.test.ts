import { fireEvent, render, screen, within } from '@testing-library/svelte';
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

  it('bulk Update is disabled until a selected mod has a pending update', async () => {
    render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    await fireEvent.click(screen.getAllByRole('checkbox', { name: /select mod/i })[0]);
    const update = screen.getByRole('button', { name: /^Update$/i });
    expect((update as HTMLButtonElement).disabled).toBe(true);
  });

  it('offers orphaned deps and uninstalls them when confirmed', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    (commands.modsFindOrphans as any) = vi.fn(async () => ({ status: 'ok', data: [{ sha1: 'b', name: 'Beta', project_id: 'Beta' }] }));
    const uninstall = ((commands.modsUninstall as any) = vi.fn(async () => ({ status: 'ok', data: null })));
    render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    await fireEvent.click(screen.getAllByRole('checkbox', { name: /select mod/i })[0]); // select Alpha (sha 'a')
    const bulkBar = screen.getByTestId('bulk-bar');
    await fireEvent.click(within(bulkBar).getByRole('button', { name: /uninstall/i }));
    // Dialog appears, lists the orphan and the "no longer be needed" copy
    expect(await screen.findByText(/no longer be needed/i)).toBeTruthy();
    // Confirm — click the dialog's Uninstall button (there are now 2 'Uninstall' buttons:
    // the bulk-bar one and the dialog one). The dialog's is inside role="dialog".
    const dialog = screen.getByRole('dialog');
    const { getByRole } = within(dialog);
    await fireEvent.click(getByRole('button', { name: /uninstall/i }));
    expect(uninstall).toHaveBeenCalledWith('inst1', 'a');
    expect(uninstall).toHaveBeenCalledWith('inst1', 'b');
  });
});
