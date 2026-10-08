import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import { EMPTY_TREE_CTX } from '$lib/mods/dep-node-state';
import DepSection from '$lib/mods/installed/DepSection.svelte';
import type { RequiredByEntry } from '$lib/mods/installed/dep-graph.svelte';

// A single installed (satisfied) dependency node. Clicking its NAME must open
// the mod's info modal (onOpenDetail), while the separate «show in the list» button jumps to the
// installed row (onJump) — both must be reachable.
const installedNode: DepTreeNode = {
  source: 'modrinth',
  project_id: 'PB',
  name: 'Bravo',
  installed: true,
  declared: 'required',
  cycle: false,
  children: [],
};

describe('dep-tree node name opens the mod detail modal', () => {
  it('clicking an installed dep node NAME calls onOpenDetail with (source, project_id)', async () => {
    const onOpenDetail = vi.fn();
    const onJump = vi.fn();
    render(DepTree, {
      props: {
        nodes: [installedNode],
        onInstall: () => {},
        onAdd: () => {},
        onJump,
        onOpenDetail,
      },
    });

    // The name button opens the modal; it must NOT trigger the jump.
    await fireEvent.click(screen.getByRole('button', { name: 'Bravo' }));
    expect(onOpenDetail).toHaveBeenCalledWith('modrinth', 'PB');
    expect(onJump).not.toHaveBeenCalled();
  });

  it('keeps a separate «show in the list» button for installed dep nodes', async () => {
    const onOpenDetail = vi.fn();
    const onJump = vi.fn();
    render(DepTree, {
      props: {
        nodes: [installedNode],
        onInstall: () => {},
        onAdd: () => {},
        onJump,
        onOpenDetail,
      },
    });

    // The name button (accessible name === the exact mod name) and the «show in the list»
    // button (accessible name from the jumpToTitle aria-label) are distinct.
    const nameBtn = screen.getByRole('button', { name: 'Bravo' });
    const arrow = screen.getByRole('button', { name: 'Show Bravo in the list' });
    expect(arrow).not.toBe(nameBtn);
    expect(arrow.querySelector('.lucide-locate-fixed')).toBeTruthy();

    await fireEvent.click(arrow);
    expect(onJump).toHaveBeenCalledWith(installedNode);
    expect(onOpenDetail).not.toHaveBeenCalled();
  });
});

describe('"required by" entries are interactive', () => {
  const root = {
    sha1: 'b',
    source: 'modrinth' as const,
    project_id: 'PB',
    name: 'Bravo',
    required: [] as DepTreeNode[],
    optional: [] as DepTreeNode[],
  };
  const requiredBy: RequiredByEntry[] = [
    { name: 'Alpha', source: 'modrinth', projectId: 'PA', sha1: 'a' },
  ];

  // Pointing at the entry marks nothing (tests/installed-cross-highlight.test.ts): the way to the
  // requiring mod's row is its «show in the list», below.
  it('clicking a "required by" entry NAME opens the requiring mod', async () => {
    const onOpenDetail = vi.fn();
    render(DepSection, {
      props: {
        root,
        requiredBy,
        onInstall: () => {},
        onJump: () => {},
        onOpenDetail,
      },
    });

    // The name button opens the requiring mod's info modal.
    await fireEvent.click(screen.getByRole('button', { name: 'Alpha' }));
    expect(onOpenDetail).toHaveBeenCalledWith('modrinth', 'PA');
  });

  it('a "required by" entry has a separate «show in the list» button that navigates to the requiring mod row', async () => {
    const onJump = vi.fn();
    const onOpenDetail = vi.fn();
    render(DepSection, {
      props: {
        root,
        requiredBy,
        onInstall: () => {},
        onJump,
        onOpenDetail,
      },
    });

    const arrow = screen.getByRole('button', { name: 'Show Alpha in the list' });
    expect(arrow.querySelector('.lucide-locate-fixed')).toBeTruthy();

    await fireEvent.click(arrow);
    // The entry knows its jar: the jump goes to that very row, never a namesake's.
    expect(onJump).toHaveBeenCalledWith({
      source: 'modrinth',
      project_id: 'PA',
      name: 'Alpha',
      sha1: 'a',
    });
    expect(onOpenDetail).not.toHaveBeenCalled();
  });

  // Spec 2026-10-07 §8: an entry is a row like the tree's — its hover fill, a click on it (not only
  // on its name) opens the mod, and the name reads as the row's text, not as a link.
  it('a click on a "required by" row opens the requiring mod; its name is no link', async () => {
    const onOpenDetail = vi.fn();
    const onJump = vi.fn();
    render(DepSection, {
      props: { root, requiredBy, onInstall: () => {}, onJump, onOpenDetail },
    });
    const name = screen.getByRole('button', { name: 'Alpha' });
    expect(name.classList).not.toContain('btn-tertiary');
    const row = name.closest('[data-required-by-row]') as HTMLElement;
    expect(row.classList).toContain('hover:bg-subtle');
    await fireEvent.click(row);
    expect(onOpenDetail.mock.calls).toEqual([['modrinth', 'PA']]);
    await fireEvent.click(screen.getByRole('button', { name: 'Show Alpha in the list' }));
    expect(onOpenDetail).toHaveBeenCalledTimes(1);
    expect(onJump).toHaveBeenCalledTimes(1);
  });

  // A dependent is installed and enabled (the list counts no other): its row carries the list's
  // switch and Remove, for its very jar, through the same handlers as the tree's.
  it('a "required by" row switches its mod off or removes it, by its jar', async () => {
    const onDisable = vi.fn();
    const onUninstall = vi.fn();
    render(DepSection, {
      props: {
        root,
        requiredBy,
        onInstall: () => {},
        onJump: () => {},
        onOpenDetail: () => {},
        treeCtx: { ...EMPTY_TREE_CTX, onDisable, onUninstall },
      },
    });
    const target = { source: 'modrinth', project_id: 'PA', name: 'Alpha', sha1: 'a' };
    const off = screen.getByRole('button', { name: 'Disable Alpha' });
    expect(off.classList).toContain('btn-icon-success');
    await fireEvent.click(off);
    expect(onDisable).toHaveBeenCalledWith(target);
    await fireEvent.click(screen.getByRole('button', { name: 'Remove Alpha' }));
    expect(onUninstall).toHaveBeenCalledWith(target);
  });

  // The same anatomy as a tree row: «show in the list» left of the name, the switch and Remove
  // last, so the columns run on from the tree above it.
  it('a "required by" row has the tree row’s columns, in its order', () => {
    render(DepSection, {
      props: { root, requiredBy, onInstall: () => {}, onJump: () => {}, onOpenDetail: () => {} },
    });
    const row = screen
      .getByRole('button', { name: 'Alpha' })
      .closest('[data-required-by-row]') as HTMLElement;
    expect(
      [...row.querySelectorAll('[data-slot]')].map((s) => s.getAttribute('data-slot')),
    ).toEqual(['locate', 'toggle', 'presence']);
  });

  // A library can be required by dozens of mods: the panel shows five and offers the rest.
  it('shows the first five dependents and the rest on request', async () => {
    const many: RequiredByEntry[] = Array.from({ length: 7 }, (_, i) => ({
      name: `Mod ${i + 1}`,
      source: 'modrinth' as const,
      projectId: `P${i + 1}`,
      sha1: `s${i + 1}`,
    }));
    render(DepSection, {
      props: {
        root,
        requiredBy: many,
        onInstall: () => {},
        onJump: () => {},
        onOpenDetail: () => {},
      },
    });
    const rows = () => document.querySelectorAll('[data-required-by-row]');
    expect(rows()).toHaveLength(5);
    await fireEvent.click(screen.getByRole('button', { name: 'Show 2 more' }));
    expect(rows()).toHaveLength(7);
    await fireEvent.click(screen.getByRole('button', { name: 'Show fewer' }));
    expect(rows()).toHaveLength(5);
  });
});

// The graph seeds every path with the panel's own mod, so it comes back one level down or deeper
// (spec 2026-10-07 D5): the section names it for every level of its trees.
describe('DepSection tells its trees which mod is its own', () => {
  it('a nested edge back to the panel’s mod reads «this mod»', async () => {
    render(DepSection, {
      props: {
        root: {
          sha1: 'a',
          source: 'modrinth' as const,
          project_id: 'PA',
          name: 'Alpha',
          required: [
            {
              ...installedNode,
              children: [{ ...installedNode, project_id: 'PA', name: 'Alpha', cycle: true }],
            },
          ],
          optional: [],
        },
        requiredBy: [],
        onInstall: () => {},
        onJump: () => {},
        onOpenDetail: () => {},
      },
    });
    expect(screen.getByText('this mod')).toBeTruthy();
    expect(screen.queryByText('expanded above')).toBeNull();
  });
});
