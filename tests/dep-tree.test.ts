import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';

const tree: DepTreeNode[] = [
  {
    source: 'modrinth',
    project_id: 'arch',
    name: 'Architectury',
    status: 'missing_required',
    cycle: false,
    children: [],
  },
  {
    source: 'modrinth',
    project_id: 'night',
    name: 'Night',
    status: 'satisfied',
    cycle: false,
    children: [],
  },
];

describe('DepTree', () => {
  it('renders missing nodes with an Install action', async () => {
    const onInstall = vi.fn();
    render(DepTree, {
      props: { nodes: tree, hoveredKey: null, onHover: () => {}, onInstall, onAdd: () => {} },
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
      },
    });
    expect(screen.queryByRole('button', { name: /install night/i })).toBeNull();
  });
});
