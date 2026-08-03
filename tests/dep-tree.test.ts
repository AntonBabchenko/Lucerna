import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';

const tree: DepTreeNode[] = [
  {
    source: 'modrinth',
    project_id: 'arch',
    name: 'Architectury',
    installed: false,
    declared: 'required',
    cycle: false,
    children: [],
  },
  {
    source: 'modrinth',
    project_id: 'night',
    name: 'Night',
    installed: true,
    declared: 'required',
    cycle: false,
    children: [],
  },
];

describe('DepTree', () => {
  it('renders missing nodes with an Install action', async () => {
    const onInstall = vi.fn();
    render(DepTree, {
      props: {
        nodes: tree,
        hoveredKey: null,
        onHover: () => {},
        onInstall,
        onAdd: () => {},
        onOpenDetail: () => {},
      },
    });
    expect(screen.getByText('Architectury')).toBeTruthy();
    const install = screen.getByRole('button', { name: /install architectury/i });
    await fireEvent.click(install);
    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'arch' }));
  });

  it('does not render an Install action for satisfied nodes', () => {
    render(DepTree, {
      props: {
        nodes: tree,
        hoveredKey: null,
        onHover: () => {},
        onInstall: () => {},
        onAdd: () => {},
        onOpenDetail: () => {},
      },
    });
    expect(screen.queryByRole('button', { name: /install night/i })).toBeNull();
  });

  it('opens the mod detail modal when a node name is clicked', async () => {
    const onOpenDetail = vi.fn();
    const onJump = vi.fn();
    render(DepTree, {
      props: {
        nodes: tree,
        hoveredKey: null,
        onHover: () => {},
        onInstall: () => {},
        onAdd: () => {},
        onJump,
        onOpenDetail,
      },
    });
    // Clicking the NAME opens the info modal for any node (installed or not).
    await fireEvent.click(screen.getByRole('button', { name: 'Night' }));
    expect(onOpenDetail).toHaveBeenCalledWith('modrinth', 'night');
    expect(onJump).not.toHaveBeenCalled();
  });

  it('keeps a separate ↗ jump button for installed (satisfied) nodes', async () => {
    const onJump = vi.fn();
    const onOpenDetail = vi.fn();
    render(DepTree, {
      props: {
        nodes: tree,
        hoveredKey: null,
        onHover: () => {},
        onInstall: () => {},
        onAdd: () => {},
        onJump,
        onOpenDetail,
      },
    });
    // 'Night' is satisfied (installed) → a distinct ↗ button jumps to its row.
    await fireEvent.click(screen.getByRole('button', { name: 'Show Night in the list' }));
    expect(onJump).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'night' }));
    expect(onOpenDetail).not.toHaveBeenCalled();
  });
});
