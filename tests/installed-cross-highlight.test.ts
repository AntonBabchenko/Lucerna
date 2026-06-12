import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// Mirrors the vi.hoisted listener registry pattern from
// installed-mods-view.test.ts so the (hoisted) vi.mock factory can register
// the event callbacks the view subscribes to on mount.
const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modUninstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

// Two installed platform mods: A (sha1 'a', project PA) requires B; B (sha1 'b',
// project PB) stands alone. The dependency graph reports A as a root whose
// single required child is the satisfied node B. The factory helpers live in
// vi.hoisted so they exist before the (also hoisted) vi.mock factory runs.
const { mod, proj } = vi.hoisted(() => ({
  mod: (sha1: string, projectId: string, name: string, requires: string[] = []) => ({
    filename: `${sha1}.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires,
  }),
  proj: (projectId: string, name: string) => ({
    status: 'ok',
    data: {
      summary: {
        source: 'modrinth',
        project_id: projectId,
        slug: name.toLowerCase(),
        name,
        summary: '',
        icon_url: null,
        downloads: 1,
        author: 'x',
        updated_at: null,
      },
      description: '',
      website_url: null,
    },
  }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [mod('a', 'PA', 'Alpha', ['PB']), mod('b', 'PB', 'Bravo')],
    }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsProject: vi
      .fn()
      .mockImplementation((_src: string, id: string) =>
        Promise.resolve(id === 'PA' ? proj('PA', 'Alpha') : proj('PB', 'Bravo')),
      ),
    modsDependencyGraph: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        roots: [
          {
            sha1: 'a',
            name: 'Alpha',
            required: [
              {
                source: 'modrinth',
                project_id: 'PB',
                name: 'Bravo',
                status: 'satisfied',
                cycle: false,
                children: [],
              },
            ],
            optional: [],
          },
          { sha1: 'b', name: 'Bravo', required: [], optional: [] },
        ],
      },
    }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: {
      listen: (cb: () => void) => {
        listeners.modInstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modUninstalled: {
      listen: (cb: () => void) => {
        listeners.modUninstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modToggle: {
      listen: (cb: () => void) => {
        listeners.modToggle = cb;
        return Promise.resolve(() => {});
      },
    },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import InstalledModsView from '$lib/mods/InstalledModsView.svelte';

describe('hover cross-highlight across dep-tree nodes and rows', () => {
  it("hovering a dep-tree node highlights the same mod's main row", async () => {
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });

    // Wait for rows + the async dep-graph load: once the graph resolves, A's
    // row shows a "1 dep" chip button (depTotal > 0). Scope the search to A's
    // row so we don't match the toolbar's "Re-check deps" button (which also
    // contains "dep" but only re-resolves the graph — it does not expand).
    const expandBtn = await waitFor(() => {
      const aRow = document.querySelector('[data-mod-row="modrinth:PA"]');
      const btn = aRow
        ? [...aRow.querySelectorAll('button')].find((b) => /dep/i.test(b.textContent ?? ''))
        : undefined;
      if (!btn) throw new Error('dep-chip not rendered yet');
      return btn as HTMLButtonElement;
    });

    // Expand A's dep tree so B's node renders inside the DepSection.
    await fireEvent.click(expandBtn);

    // After expansion there are TWO elements keyed modrinth:PB — B's main row
    // (which carries BOTH data-mod-key and data-mod-row) and the dep-tree node
    // (data-mod-key only). Pick the dep-tree node: the one without data-mod-row.
    const depNode = await waitFor(() => {
      const node = [...document.querySelectorAll('[data-mod-key="modrinth:PB"]')].find(
        (el) => !el.hasAttribute('data-mod-row'),
      );
      if (!node) throw new Error('dep-tree node not rendered yet');
      return node as HTMLElement;
    });

    // Hovering the dep-tree node sets hoveredKey = "modrinth:PB" via onHover,
    // which propagates back to B's main row's class:bg-highlight binding.
    await fireEvent.mouseEnter(depNode);

    const bRow = document.querySelector('[data-mod-row="modrinth:PB"]');
    expect(bRow?.className).toContain('bg-highlight');

    // The wrapper highlight alone was visually masked: ModCard's list-row paints
    // an opaque bg-surface that covers the parent's bg-highlight. The fix routes
    // the highlight into ModCard via the `highlighted` prop, so the inner row
    // itself must now carry bg-highlight (this is the part that was broken).
    const bInnerRow = document.querySelector(
      '[data-mod-row="modrinth:PB"] [data-testid="card-list-row"]',
    );
    expect(bInnerRow?.className).toContain('bg-highlight');
  });

  it('hovering a mod row highlights only that row, not its expanded dep node', async () => {
    // Regression guard for the cross-highlight conflict: when A's dep tree is
    // expanded, the mod-row hover region and the dep-tree node's hover used to
    // share the same outer element and fight over hoveredKey. The fix moves the
    // DepSection OUT of the hover region (sibling), so hovering A's row sets
    // hoveredKey = A's rowKey and the B dep-node (key PB ≠ PA) stays un-highlighted.
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });

    const expandBtn = await waitFor(() => {
      const aRow = document.querySelector('[data-mod-row="modrinth:PA"]');
      const btn = aRow
        ? [...aRow.querySelectorAll('button')].find((b) => /dep/i.test(b.textContent ?? ''))
        : undefined;
      if (!btn) throw new Error('dep-chip not rendered yet');
      return btn as HTMLButtonElement;
    });

    // Expand A's dep tree so B's node renders inside the (now sibling) DepSection.
    await fireEvent.click(expandBtn);

    // The B dep-tree node (data-mod-key only, no data-mod-row) must exist.
    const depNode = await waitFor(() => {
      const node = [...document.querySelectorAll('[data-mod-key="modrinth:PB"]')].find(
        (el) => !el.hasAttribute('data-mod-row'),
      );
      if (!node) throw new Error('dep-tree node not rendered yet');
      return node as HTMLElement;
    });

    // Hover A's MOD ROW (the inner hover region). This sets hoveredKey = A's key.
    const aRow = document.querySelector('[data-mod-row="modrinth:PA"]') as HTMLElement;
    await fireEvent.mouseEnter(aRow);

    // A's row highlights...
    expect(aRow.className).toContain('bg-highlight');
    // ...but the B dep-node does NOT (its key is PB ≠ PA).
    expect(depNode.className).not.toContain('bg-highlight');

    // The load-bearing structural guard: the expanded dep node must NOT be a
    // descendant of A's hover-region element. On the OLD structure the
    // DepSection was nested INSIDE the element carrying data-mod-row +
    // onmouseenter, so A's bg-highlight covered the whole dep section and the
    // node's own per-node hover fought the row's hover over the shared
    // hoveredKey. The fix makes DepSection a SIBLING of the hover region — so
    // aRow no longer contains the dep node. This assertion fails on the old
    // structure (aRow.contains(depNode) === true) and passes on the new one.
    expect(aRow.contains(depNode)).toBe(false);
  });
});
