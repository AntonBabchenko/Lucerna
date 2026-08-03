import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import { isClaimDismissed } from '$lib/mods/dep-claim-dismiss';
import DepSection from '$lib/mods/installed/DepSection.svelte';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

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

describe('DepSection — the author is credited, and the user can settle a claim', () => {
  const modRef = { source: 'modrinth' as const, project_id: 'PA' };
  const absentRequired = (pid: string, name: string): DepTreeNode => ({
    source: 'modrinth',
    project_id: pid,
    name,
    installed: false,
    declared: 'required',
    cycle: false,
    children: [],
  });
  const root = () => ({
    sha1: 'a',
    source: 'modrinth' as const,
    project_id: 'PA',
    name: 'Alpha',
    required: [absentRequired('PB', 'Stylish Effects'), absentRequired('PC', 'Some Lib')],
    optional: [],
  });
  const sectionProps = () => ({
    root: root(),
    requiredBy: [],
    hoveredKey: null,
    onHover: () => {},
    onInstall: () => {},
    onJump: () => {},
    onOpenDetail: () => {},
  });

  beforeEach(() => diagnosisDismiss.reset());

  it('heads the section with attribution rather than an assertion', () => {
    render(DepSection, { props: sectionProps() });
    expect(screen.getByText(/author marked required/i)).toBeTruthy();
    expect(screen.queryByText(/^Requires$/)).toBeNull();
  });

  it('dismissing one claim leaves the sibling claim visible', async () => {
    render(DepSection, { props: sectionProps() });
    expect(screen.getAllByTestId('claim-dismiss')).toHaveLength(2);
    expect(screen.queryByTestId('claim-restore')).toBeNull();

    await fireEvent.click(screen.getAllByTestId('claim-dismiss')[0]);

    expect(screen.getAllByTestId('claim-dismiss')).toHaveLength(1);
    expect(screen.getByText('Some Lib')).toBeTruthy();
    expect(screen.queryByText('Stylish Effects')).toBeNull();
    expect(screen.getByTestId('claim-restore').textContent).toMatch(/1/);
    // Exactly the acknowledged pair is settled — not the mod, not the instance.
    expect(isClaimDismissed(modRef, { source: 'modrinth', project_id: 'PB' })).toBe(true);
    expect(isClaimDismissed(modRef, { source: 'modrinth', project_id: 'PC' })).toBe(false);
  });

  it('restores the hidden claims', async () => {
    render(DepSection, { props: sectionProps() });
    await fireEvent.click(screen.getAllByTestId('claim-dismiss')[0]);
    await fireEvent.click(screen.getByTestId('claim-restore'));
    expect(screen.getAllByTestId('claim-dismiss')).toHaveLength(2);
    expect(screen.queryByTestId('claim-restore')).toBeNull();
  });
});
