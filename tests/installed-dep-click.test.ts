import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import DepSection from '$lib/mods/installed/DepSection.svelte';
import type { RequiredByEntry } from '$lib/mods/installed/dep-graph.svelte';

// A single installed (satisfied) dependency node. Clicking its NAME must open
// the mod's info modal (onOpenDetail), while the separate ↗ button jumps to the
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

  it('keeps a separate ↗ jump button for installed dep nodes', async () => {
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

    // The name button (accessible name === the exact mod name) and the ↗ jump
    // button (accessible name from the jumpToTitle aria-label) are distinct.
    const nameBtn = screen.getByRole('button', { name: 'Bravo' });
    const arrow = screen.getByRole('button', { name: 'Show Bravo in the list' });
    expect(arrow).not.toBe(nameBtn);
    expect(arrow.querySelector('.lucide-arrow-up-right')).toBeTruthy();

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
  // requiring mod's row is its ↗, below.
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

  it('a "required by" entry has a separate ↗ jump button that navigates to the requiring mod row', async () => {
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
    expect(arrow.querySelector('.lucide-arrow-up-right')).toBeTruthy();

    await fireEvent.click(arrow);
    expect(onJump).toHaveBeenCalledWith({ source: 'modrinth', project_id: 'PA' });
    expect(onOpenDetail).not.toHaveBeenCalled();
  });
});
