import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepsUnknown, DepTreeNode, DepViolation, PreflightReport } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import { type DepTreeCtx, EMPTY_TREE_CTX } from '$lib/mods/dep-node-state';
import DepSection from '$lib/mods/installed/DepSection.svelte';
import { describedText } from './test-utils/aria';
import { rawRangeDesc } from './test-utils/range-desc';

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
    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'arch' }), null);
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

// The install path records the edge on the mod that declared the dependency (spec §5.6), so every
// Install and Add says which mod that is: the row's own at the top, an installed parent below it,
// and none under an absent parent — nothing installed declared what sits there.
describe('DepTree — an Install names the mod that declared the dependency', () => {
  it('names the level dependent at the top, the installed parent below, none under an absent one', async () => {
    const onInstall = vi.fn();
    const onAdd = vi.fn();
    const nodes: DepTreeNode[] = [
      leaf('night', { name: 'Night', children: [leaf('lib', { name: 'Lib', installed: false })] }),
      leaf('arch', {
        name: 'Arch',
        installed: false,
        children: [leaf('sub', { name: 'Sub', installed: false })],
      }),
      leaf('extra', { name: 'Extra', installed: false, declared: 'optional' }),
    ];
    const ctx: DepTreeCtx = {
      ...EMPTY_TREE_CTX,
      enabledShaOf: (k) => (k === 'modrinth:night' ? 'night-sha' : null),
    };
    render(DepTree, {
      props: { ...treeProps({ nodes, onInstall, onAdd, ctx }), dependentSha1: 'row-sha' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Install Arch' }));
    expect(onInstall).toHaveBeenLastCalledWith(
      expect.objectContaining({ project_id: 'arch' }),
      'row-sha',
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Install Lib' }));
    expect(onInstall).toHaveBeenLastCalledWith(
      expect.objectContaining({ project_id: 'lib' }),
      'night-sha',
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Install Sub' }));
    expect(onInstall).toHaveBeenLastCalledWith(
      expect.objectContaining({ project_id: 'sub' }),
      null,
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Add Extra' }));
    expect(onAdd).toHaveBeenLastCalledWith(
      expect.objectContaining({ project_id: 'extra' }),
      'row-sha',
    );
  });
});

const leaf = (pid: string, over: Partial<DepTreeNode> = {}): DepTreeNode => ({
  source: 'modrinth',
  project_id: pid,
  name: pid.toUpperCase(),
  installed: true,
  declared: 'required',
  cycle: false,
  children: [],
  ...over,
});
const treeProps = (over: Record<string, unknown> = {}) => ({
  nodes: [leaf('a', { children: [leaf('b', { children: [leaf('c')] })] }), leaf('d')],
  hoveredKey: null,
  onHover: () => {},
  onInstall: () => {},
  onAdd: () => {},
  onOpenDetail: () => {},
  ...over,
});
const item = (name: string) =>
  screen.getByText(name, { selector: 'button' }).closest('[role="treeitem"]') as HTMLElement;

describe('DepSection — headings state the relation; nothing to dismiss', () => {
  const absent = (pid: string, name: string): DepTreeNode => leaf(pid, { name, installed: false });
  const sectionProps = () => ({
    root: {
      sha1: 'a',
      source: 'modrinth' as const,
      project_id: 'PA',
      name: 'Alpha',
      required: [absent('PB', 'Stylish Effects')],
      optional: [{ ...absent('PO', 'Extras'), declared: 'optional' as const }],
    },
    requiredBy: [{ name: 'Gamma', source: 'modrinth' as const, projectId: 'PG', sha1: 'g' }],
    hoveredKey: null,
    onHover: () => {},
    onInstall: () => {},
    onJump: () => {},
    onOpenDetail: () => {},
  });

  it('heads the lists «Requires» / «Optional» / «Required by» — the author-claim wording is gone', () => {
    render(DepSection, { props: sectionProps() });
    expect(screen.getByText('Requires')).toBeTruthy();
    expect(screen.getByText('Optional')).toBeTruthy();
    expect(screen.getByText('Required by')).toBeTruthy();
    expect(screen.queryByText(/author marked/i)).toBeNull();
  });

  // Its muted headings and cycle marker were 4.46:1 on the old bg-subtle/40 tint in the light
  // theme; on the surface every text colour the section uses clears AA in both themes.
  it('sits on the surface, where its muted text clears AA', () => {
    const { container } = render(DepSection, { props: sectionProps() });
    const panel = container.firstElementChild as HTMLElement;
    expect(panel.classList.contains('bg-surface')).toBe(true);
    expect([...panel.classList].some((c) => c.startsWith('bg-subtle'))).toBe(false);
  });

  it('offers no claim dismissal and no hidden-claims line', () => {
    render(DepSection, { props: sectionProps() });
    expect(screen.queryByTestId('claim-dismiss')).toBeNull();
    expect(screen.queryByTestId('claim-restore')).toBeNull();
  });

  it('labels each tree by its heading', () => {
    render(DepSection, { props: sectionProps() });
    const trees = screen.getAllByRole('tree');
    expect(trees).toHaveLength(2);
    expect(trees[0]?.getAttribute('aria-labelledby')).toBe('dep-req-a');
    expect(document.getElementById('dep-req-a')?.textContent).toContain('Requires');
    expect(trees[1]?.getAttribute('aria-labelledby')).toBe('dep-opt-a');
    expect(document.getElementById('dep-opt-a')?.textContent).toContain('Optional');
  });
});

// What the platform could not describe is not "requires nothing" (fallback discipline: "could
// not tell" ≠ "absent"). The tree says so, and why, where the requirements would be.
describe('DepSection / DepTree — dependencies the platform could not describe', () => {
  const sectionOf = (deps_unknown: DepsUnknown) => ({
    root: {
      sha1: 'a',
      source: 'modrinth' as const,
      project_id: 'PA',
      name: 'Alpha',
      required: [],
      optional: [],
      deps_unknown,
    },
    requiredBy: [{ name: 'Gamma', source: 'modrinth' as const, projectId: 'PG', sha1: 'g' }],
    hoveredKey: null,
    onHover: () => {},
    onInstall: () => {},
    onJump: () => {},
    onOpenDetail: () => {},
  });

  it('says a mod’s dependencies are unknown, and why — never an empty «Requires»', () => {
    render(DepSection, { props: sectionOf('unreachable') });
    expect(screen.getByText('Dependencies unknown — the platform is unavailable')).toBeTruthy();
    expect(screen.queryByText('Requires')).toBeNull();
    expect(screen.getByText('Required by')).toBeTruthy();
  });

  it('names an unidentified installed version as the reason', () => {
    render(DepSection, { props: sectionOf('unidentified') });
    expect(
      screen.getByText(
        "Dependencies unknown — the installed version isn't identified on the platform",
      ),
    ).toBeTruthy();
  });

  it('marks a nested installed mod whose dependencies are unknown', () => {
    render(DepTree, {
      props: treeProps({ nodes: [leaf('x', { name: 'Xaero', deps_unknown: 'unreachable' })] }),
    });
    const el = screen.getByRole('treeitem', { name: 'Xaero' });
    expect(describedText(el)).toBe('installed dependencies unknown');
  });
});

describe('DepTree — each absent dependency says what the loader does', () => {
  const miss = (depId: string): DepViolation => ({
    kind: 'missing_required',
    dependent_name: 'Alpha',
    dependent_sha1: 'a',
    dep_id: depId,
    needed: '',
    needed_desc: rawRangeDesc(''),
    installed_version: null,
    provider_project: null,
    provider_sha1: null,
    family: null,
  });
  const ctx = (report: PreflightReport | null, over: Partial<DepTreeCtx> = {}): DepTreeCtx => ({
    ...EMPTY_TREE_CTX,
    report,
    projectOf: (_s, depId) => (depId === 'balm' ? { source: 'modrinth', project_id: 'pb' } : null),
    ...over,
  });
  const one = (n: DepTreeNode, c: DepTreeCtx) =>
    render(DepTree, { props: treeProps({ nodes: [n], dependentSha1: 'a', ctx: c }) });
  const balm = leaf('pb', { name: 'Balm', installed: false });

  it('red when the loader requires it', () => {
    one(balm, ctx({ violations: [miss('balm')] }));
    expect(item('Balm').getAttribute('data-node-state')).toBe('loader_required');
    expect(
      screen.getByText("not installed — the game won't start without it").closest('.text-danger'),
    ).not.toBeNull();
  });

  it('neutral, and says the mod starts without it, when the loader asked for nothing', () => {
    one(balm, ctx({ violations: [] }));
    expect(item('Balm').getAttribute('data-node-state')).toBe('platform_only');
    expect(
      screen.getByText('not installed · per the platform; the mod starts without it'),
    ).toBeTruthy();
    expect(item('Balm').querySelector('.text-danger')).toBeNull();
  });

  it('makes no loader claim for an unjudged dependent', () => {
    one(balm, ctx({ violations: [], unjudged: ['a'] }));
    expect(screen.getByText('not installed · per the platform')).toBeTruthy();
    expect(screen.queryByText(/starts without it|won't start/)).toBeNull();
  });

  it('a disabled dependency offers Enable', async () => {
    const onEnable = vi.fn();
    const x = leaf('px', { name: 'Xaero', installed: false, disabled: true });
    one(x, ctx({ violations: [] }, { onEnable }));
    expect(screen.getByText('disabled')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Enable Xaero' }));
    expect(onEnable).toHaveBeenCalledWith(x);
  });

  // The item's name is the mod; what the tree says about it is its description — the state is
  // never hidden behind a label, and nested items are not read as part of their parent's name.
  it('names each item by its mod and describes it by what the tree says about it', () => {
    one(balm, ctx({ violations: [miss('balm')] }));
    const el = screen.getByRole('treeitem', { name: 'Balm' });
    expect(describedText(el)).toBe("not installed — the game won't start without it");
  });

  // A version mismatch had no action: its fix is the planner's (spec §6.5), asked about the
  // conflict behind the mark — the host names it, the tree only offers it.
  it('a version mismatch offers «Fix…», which asks the planner about its conflict', async () => {
    const onPlan = vi.fn();
    const conflict: DepViolation = {
      ...miss('sodium'),
      kind: 'version_out_of_range',
      provider_project: { source: 'modrinth', project_id: 'ps', version_id: null },
    };
    const conflictOf = vi.fn(() => conflict);
    const sodium = leaf('ps', { name: 'Sodium' });
    render(DepTree, {
      props: treeProps({
        nodes: [sodium],
        dependentSha1: 'a',
        outOfRangeKeys: new Set(['modrinth:ps']),
        ctx: ctx({ violations: [conflict] }, { conflictOf, onPlan }),
      }),
    });
    expect(screen.getByText('version mismatch')).toBeTruthy();
    const fix = screen.getByRole('button', { name: 'Fix the version conflict with Sodium' });
    // Its visible text is the panel's «Fix…»; the name only adds which node it is for.
    expect(fix.textContent?.trim()).toBe('Fix…');
    await fireEvent.click(fix);
    expect(conflictOf).toHaveBeenCalledWith(sodium, 'a');
    expect(onPlan).toHaveBeenCalledWith(conflict);
  });

  it('offers no «Fix…» where the host names no conflict to plan', () => {
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('ps', { name: 'Sodium' })],
        dependentSha1: 'a',
        outOfRangeKeys: new Set(['modrinth:ps']),
        ctx: ctx({ violations: [] }),
      }),
    });
    expect(screen.getByText('version mismatch')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /fix/i })).toBeNull();
  });
});

describe('DepTree — a WAI-ARIA tree', () => {
  it('one tree of treeitems in groups; top level open, deeper branches closed', () => {
    render(DepTree, { props: treeProps() });
    expect(screen.getAllByRole('tree')).toHaveLength(1);
    expect(item('A').getAttribute('aria-expanded')).toBe('true');
    expect(item('B').getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('C')).toBeNull();
    expect(item('D').hasAttribute('aria-expanded')).toBe(false);
    expect(item('B').parentElement?.getAttribute('role')).toBe('group');
  });

  it('keeps one tab stop and moves it with Up/Down', async () => {
    render(DepTree, { props: treeProps() });
    const stops = screen
      .getAllByRole('treeitem')
      .filter((el) => el.getAttribute('tabindex') === '0');
    expect(stops).toEqual([item('A')]);
    item('A').focus();
    await fireEvent.keyDown(item('A'), { key: 'ArrowDown' });
    expect(document.activeElement).toBe(item('B'));
    expect(item('A').getAttribute('tabindex')).toBe('-1');
    await fireEvent.keyDown(item('B'), { key: 'ArrowUp' });
    expect(document.activeElement).toBe(item('A'));
  });

  // Tab from the tree's stop reaches that item's own actions, never another item's.
  it('an item’s buttons follow its tab stop', () => {
    render(DepTree, { props: treeProps() });
    const tabIndexes = (el: HTMLElement) =>
      [...(el.firstElementChild?.querySelectorAll('button') ?? [])].map((b) =>
        b.getAttribute('tabindex'),
      );
    // name, ↗ — the chevron is a mouse affordance, never a tab stop
    expect(tabIndexes(item('A')).filter((t) => t !== '-1')).toEqual(['0', '0']);
    expect(tabIndexes(item('D')).every((t) => t === '-1')).toBe(true);
  });

  it('Right opens a closed branch; Left closes it, then climbs to the parent', async () => {
    render(DepTree, { props: treeProps() });
    item('B').focus();
    await fireEvent.keyDown(item('B'), { key: 'ArrowRight' });
    expect(item('B').getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('C')).toBeTruthy();
    await fireEvent.keyDown(item('B'), { key: 'ArrowLeft' });
    expect(item('B').getAttribute('aria-expanded')).toBe('false');
    await fireEvent.keyDown(item('B'), { key: 'ArrowLeft' });
    expect(document.activeElement).toBe(item('A'));
  });

  it('Home/End reach the ends; Enter opens the mod', async () => {
    const onOpenDetail = vi.fn();
    render(DepTree, { props: treeProps({ onOpenDetail }) });
    item('A').focus();
    await fireEvent.keyDown(item('A'), { key: 'End' });
    expect(document.activeElement).toBe(item('D'));
    await fireEvent.keyDown(item('D'), { key: 'Home' });
    expect(document.activeElement).toBe(item('A'));
    await fireEvent.keyDown(item('A'), { key: 'Enter' });
    expect(onOpenDetail).toHaveBeenCalledWith('modrinth', 'a');
  });

  // Not the first item: the branch that hid the stop takes it.
  it('a branch that closes around the tab stop takes the stop', async () => {
    render(DepTree, {
      props: treeProps({ nodes: [leaf('a'), leaf('d', { children: [leaf('e')] })] }),
    });
    item('A').focus();
    await fireEvent.keyDown(item('A'), { key: 'End' }); // E holds the stop
    expect(item('E').getAttribute('tabindex')).toBe('0');
    const chevron = item('D').querySelector<HTMLElement>('[data-tree-toggle]');
    await fireEvent.click(chevron as HTMLElement); // the mouse closes D around it
    expect(item('D').getAttribute('aria-expanded')).toBe('false');
    expect(item('D').getAttribute('tabindex')).toBe('0');
    expect(item('A').getAttribute('tabindex')).toBe('-1');
  });

  // The graph is re-resolved after every mod change; the item holding the stop may be gone.
  it('the first item takes the stop when the one holding it is gone', async () => {
    const { rerender } = render(DepTree, { props: treeProps() });
    item('A').focus();
    await fireEvent.keyDown(item('A'), { key: 'End' }); // D holds the stop
    expect(item('D').getAttribute('tabindex')).toBe('0');
    await rerender(treeProps({ nodes: [leaf('a'), leaf('e')] }));
    expect(item('A').getAttribute('tabindex')).toBe('0');
    expect(item('E').getAttribute('tabindex')).toBe('-1');
  });

  it('hover highlights but never selects', () => {
    render(DepTree, { props: treeProps({ hoveredKey: 'modrinth:d' }) });
    expect(item('D').getAttribute('aria-selected')).toBe('false');
    expect(item('D').querySelector('.tree-row')?.classList.contains('dep-highlight')).toBe(true);
  });
});
