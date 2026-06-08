import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { depGraphCache } from '$lib/mods/dep-graph-cache';
import InstalledModsView from '$lib/mods/InstalledModsView.svelte';

vi.mock('$lib/ipc/bindings', async (orig) => {
  const actual = await orig<typeof import('$lib/ipc/bindings')>();
  const mod = (sha1: string, name: string, enabled = true) => ({
    filename: `${name}.jar`,
    sha1,
    source: 'modrinth',
    project_id: name,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled,
    enrich_attempted: false,
    requires: [],
  });
  return {
    ...actual,
    commands: {
      ...actual.commands,
      modsListInstalled: vi.fn(async () => ({
        status: 'ok',
        data: [mod('a', 'Alpha'), mod('b', 'Beta', false)],
      })),
      modsPackOriginSummary: vi.fn(async () => ({ status: 'ok', data: null })),
      modsProject: vi.fn(async (_s: unknown, id: string) => ({
        status: 'ok',
        data: {
          summary: {
            source: 'modrinth',
            project_id: id,
            slug: id,
            name: id,
            summary: '',
            icon_url: null,
            downloads: 0,
            author: 'x',
            updated_at: null,
          },
        },
      })),
      modsDependencyGraph: vi.fn(async () => ({ status: 'ok', data: { roots: [] } })),
      scanInstanceModCompat: vi.fn(async () => ({ status: 'ok', data: [] })),
      checkInstanceModCompat: vi.fn(async () => ({ status: 'ok', data: [] })),
    },
    events: Object.fromEntries(
      Object.keys(actual.events).map((k) => [k, { listen: async () => () => {} }]),
    ),
  };
});

const props = { instanceId: 'inst1', mcVersion: '1.20.1', loader: 'fabric' as const };

// The dependency-graph cache is a process-lifetime singleton shared across
// tests. Without clearing it, the first test seeds `inst1` with an empty graph
// and later tests (which swap the modsDependencyGraph mock) re-use that stale
// cached graph instead of re-resolving — so dep chips never render.
beforeEach(() => {
  depGraphCache.delete('inst1');
});

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
    (commands.modsFindOrphans as any) = vi.fn(async () => ({
      status: 'ok',
      data: [{ sha1: 'b', name: 'Beta', project_id: 'Beta' }],
    }));
    (commands.modsUninstall as any) = vi.fn(async () => ({ status: 'ok', data: null }));
    const uninstall = vi.mocked(commands.modsUninstall);
    render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    await fireEvent.click(screen.getAllByRole('checkbox', { name: /select mod/i })[0]); // select Alpha (sha 'a')
    const bulkBar = screen.getByTestId('bulk-bar');
    await fireEvent.click(within(bulkBar).getByRole('button', { name: /uninstall/i }));
    // Dialog appears and offers the orphan (opt-in, default unchecked).
    expect(await screen.findByText(/also remove/i)).toBeTruthy();
    const dialog = screen.getByRole('dialog');
    const { getByRole } = within(dialog);
    const orphanBox = getByRole('checkbox') as HTMLInputElement;
    expect(orphanBox.checked).toBe(false); // destructive secondary removal is opt-in
    // Opt in to removing the orphan, then confirm → both the selected mod and
    // the now-ticked orphan are uninstalled.
    await fireEvent.click(orphanBox);
    await fireEvent.click(getByRole('button', { name: /uninstall/i }));
    expect(uninstall).toHaveBeenCalledWith('inst1', 'a');
    expect(uninstall).toHaveBeenCalledWith('inst1', 'b');
  });

  it('leaves orphaned deps installed when not opted in', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    (commands.modsFindOrphans as any) = vi.fn(async () => ({
      status: 'ok',
      data: [{ sha1: 'b', name: 'Beta', project_id: 'Beta' }],
    }));
    (commands.modsUninstall as any) = vi.fn(async () => ({ status: 'ok', data: null }));
    const uninstall = vi.mocked(commands.modsUninstall);
    render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    await fireEvent.click(screen.getAllByRole('checkbox', { name: /select mod/i })[0]);
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: /uninstall/i }),
    );
    const dialog = await screen.findByRole('dialog');
    // Confirm without ticking the orphan → only the selected mod is removed.
    await fireEvent.click(within(dialog).getByRole('button', { name: /uninstall/i }));
    expect(uninstall).toHaveBeenCalledWith('inst1', 'a');
    expect(uninstall).not.toHaveBeenCalledWith('inst1', 'b');
  });

  it('hovering a row highlights every occurrence of that mod', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    (commands.modsDependencyGraph as any) = vi.fn(async () => ({
      status: 'ok',
      data: {
        roots: [
          {
            sha1: 'a',
            source: 'modrinth',
            project_id: 'Alpha',
            name: 'Alpha',
            required: [],
            optional: [],
          },
          {
            sha1: 'b',
            source: 'modrinth',
            project_id: 'Beta',
            name: 'Beta',
            required: [
              {
                source: 'modrinth',
                project_id: 'Alpha',
                name: 'Alpha',
                status: 'satisfied',
                cycle: false,
                children: [],
              },
            ],
            optional: [],
          },
        ],
      },
    }));
    const { container } = render(InstalledModsView, { props });
    await screen.findByText('Alpha');
    // Beta has a dep chip ("required by"/"1 dep") — expand Beta so the Alpha tree node is on screen.
    // Find Beta's expand chip and click it.
    const betaExpand = await screen.findByRole('button', { name: /1 dep/i });
    await fireEvent.click(betaExpand);
    // Now hover Alpha's own ROW (the container wrapping its ModCard), keyed modrinth:Alpha.
    const alphaOccurrences = container.querySelectorAll('[data-mod-key="modrinth:Alpha"]');
    expect(alphaOccurrences.length).toBeGreaterThanOrEqual(2); // row + tree node
    await fireEvent.mouseEnter(alphaOccurrences[0] as Element);
    const highlighted = container.querySelectorAll('[data-mod-key="modrinth:Alpha"].bg-highlight');
    expect(highlighted.length).toBeGreaterThanOrEqual(2);
  });

  // Kept last: it overrides modsListInstalled to return many mods, which would
  // otherwise leak into the Alpha/Beta-based tests above.
  it('paginates the installed list (page size + Next)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const many = Array.from({ length: 25 }, (_, i) => ({
      filename: `Mod${i}.jar`,
      sha1: `s${i}`,
      source: 'modrinth',
      project_id: `Mod${i}`,
      version_id: 'v',
      name: `Mod${i}`,
      version_number: '1.0',
      installed_at: '2026-01-01T00:00:00Z',
      enabled: true,
      enrich_attempted: false,
      requires: [],
    }));
    (commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: many,
    });
    render(InstalledModsView, { props });
    await screen.findByText('Mod0');
    // Switch to 20 per page → first page shows 20, with a "Page 1 of 2" pager.
    await fireEvent.click(screen.getByRole('button', { name: '20' }));
    expect(screen.getAllByRole('checkbox', { name: /select mod/i }).length).toBe(20);
    expect(screen.getByText(/page 1 of 2/i)).toBeTruthy();
    // Next → second page shows the remaining 5.
    await fireEvent.click(screen.getByRole('button', { name: /next/i }));
    expect(screen.getAllByRole('checkbox', { name: /select mod/i }).length).toBe(5);
  });
});
