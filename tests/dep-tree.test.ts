import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DepsUnknown, DepTreeNode, DepViolation, PreflightReport } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import { type DepTreeCtx, EMPTY_TREE_CTX } from '$lib/mods/dep-node-state';
import DepSection from '$lib/mods/installed/DepSection.svelte';
import { hideTooltip, TOOLTIP_ID, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
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

  it('keeps a separate «show in the list» button for installed (satisfied) nodes', async () => {
    const onJump = vi.fn();
    const onOpenDetail = vi.fn();
    render(DepTree, {
      props: {
        nodes: tree,
        onInstall: () => {},
        onAdd: () => {},
        onJump,
        onOpenDetail,
      },
    });
    // 'Night' is satisfied (installed) → a distinct «show in the list» button jumps to its row.
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

  // Alpha's own range rejects the installed Sodium: the mark is this edge's (plan §5b V1).
  const conflict: DepViolation = {
    ...miss('sodium'),
    kind: 'version_out_of_range',
    provider_project: { source: 'modrinth', project_id: 'ps', version_id: null },
  };

  // A version mismatch had no action: its fix is the planner's (spec §6.5), asked about the
  // conflict behind the mark — the host names it, the tree only offers it.
  it('a version mismatch offers «Fix…», which asks the planner about its conflict', async () => {
    const onPlan = vi.fn();
    const conflictOf = vi.fn(() => conflict);
    const sodium = leaf('ps', { name: 'Sodium' });
    render(DepTree, {
      props: treeProps({
        nodes: [sodium],
        dependentSha1: 'a',
        ctx: ctx({ violations: [conflict] }, { conflictOf, onPlan }),
      }),
    });
    expect(screen.getByText('version mismatch')).toBeTruthy();
    const fix = screen.getByRole('button', { name: 'Fix the version conflict with Sodium' });
    // Icon-only like every action in the tree (maintainer, 2026-10-07): the wrench, its name in
    // the tooltip. It belongs to the state it remedies, so it sits in the state's group, before
    // the icon columns.
    expect(fix.textContent?.trim()).toBe('');
    expect(fix.querySelector('.lucide-wrench')).toBeTruthy();
    expect(fix.closest('[data-state-group]')).toBeTruthy();
    await fireEvent.click(fix);
    expect(conflictOf).toHaveBeenCalledWith(sodium, 'a');
    expect(onPlan).toHaveBeenCalledWith(conflict);
  });

  it('offers no «Fix…» where the host names no conflict to plan', () => {
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('ps', { name: 'Sodium' })],
        dependentSha1: 'a',
        // The host's default: nothing to fix (an advisory report, say).
        ctx: ctx({ violations: [conflict] }),
      }),
    });
    expect(screen.getByText('version mismatch')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /fix/i })).toBeNull();
  });

  // 06d: Indium's range on Sodium is Indium's conflict; under a mod whose own ranges accept the
  // installed build the node is simply installed, whatever another mod's range says.
  it('another mod’s range on the same dependency marks nothing here', () => {
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('ps', { name: 'Sodium' })],
        dependentSha1: 'iris',
        ctx: ctx({ violations: [conflict] }, { conflictOf: () => conflict }),
      }),
    });
    expect(item('Sodium').getAttribute('data-node-state')).toBe('installed');
    expect(screen.queryByText('version mismatch')).toBeNull();
    expect(screen.queryByRole('button', { name: /fix/i })).toBeNull();
  });
});

// Create+ → Create Aeronautics → Sable showed «ImGuiMC не установлен [Добавить]» under the top
// «Requires» heading: a deeper level mixes what its parent requires with what it only offers,
// and only the button's verb told them apart (2026-10-02 regression, O1).
describe('DepTree — an optional dependency below the top level says so', () => {
  const nested = () =>
    render(DepTree, {
      props: treeProps({
        nodes: [
          leaf('sable', {
            name: 'Sable',
            children: [
              leaf('imgui', { name: 'ImGuiMC', installed: false, declared: 'optional' }),
              leaf('lib', { name: 'Lib', installed: false }),
            ],
          }),
        ],
      }),
    });

  it('marks it, and the mark comes first in what describes it', () => {
    nested();
    const imgui = item('ImGuiMC');
    expect(imgui.querySelector('[id$="-optional"]')?.textContent).toBe('optional');
    expect(describedText(imgui)).toBe('optional not installed');
  });

  it('leaves a required one beside it unmarked', () => {
    nested();
    expect(item('Lib').querySelector('[id$="-optional"]')).toBeNull();
    expect(describedText(item('Lib'))).not.toContain('optional');
  });

  it('needs no mark at the top level, whose heading says it', () => {
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('extra', { name: 'Extra', installed: false, declared: 'optional' })],
      }),
    });
    expect(item('Extra').querySelector('[id$="-optional"]')).toBeNull();
    expect(describedText(item('Extra'))).toBe('not installed');
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
    // name, switch, remove, show in the list — the chevron is a mouse affordance, never a tab stop
    expect(tabIndexes(item('A')).filter((t) => t !== '-1')).toEqual(['0', '0', '0', '0']);
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

  // aria-selected follows the tab stop, never the pointer (spec §6.3) — 0.25.0 selected the
  // hovered item. Pointing marks nothing either (tests/installed-cross-highlight.test.ts).
  it('pointing at an item never selects it', async () => {
    render(DepTree, { props: treeProps() });
    const row = item('D').querySelector('.tree-row') as HTMLElement;
    await fireEvent.mouseEnter(item('D'));
    await fireEvent.mouseEnter(row);
    await fireEvent.mouseOver(row);
    expect(item('D').getAttribute('aria-selected')).toBe('false');
    expect(item('A').getAttribute('aria-selected')).toBe('true');
  });
});

// Spec 2026-10-07: a node's actions are the Installed row's own icon buttons (ModCard), in
// columns that line up at every depth; a mark after the state is set apart («установлен ·
// зависимости неизвестны», not run together); a disabled node jumps to its row like an installed
// one; and every row is one height, a row with a button or a chevron in the same rhythm as one
// without.
describe('DepTree — node actions and rhythm', () => {
  it('an absent dependency’s Install and Add are the list’s icon buttons', async () => {
    const onInstall = vi.fn();
    const onAdd = vi.fn();
    const nodes = [
      leaf('arch', { name: 'Arch', installed: false }),
      leaf('extra', { name: 'Extra', installed: false, declared: 'optional' }),
    ];
    render(DepTree, { props: treeProps({ nodes, onInstall, onAdd }) });
    const install = screen.getByRole('button', { name: 'Install Arch' });
    expect(install.textContent?.trim()).toBe('');
    expect(install.classList).toContain('btn-icon');
    expect(install.classList).toContain('btn-icon-sm');
    expect(install.classList).toContain('!text-accent');
    expect(install.querySelector('.lucide-download')).toBeTruthy();
    const add = screen.getByRole('button', { name: 'Add Extra' });
    await fireEvent.click(add);
    expect(onAdd).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'extra' }), null);
    await fireEvent.click(install);
    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'arch' }), null);
  });

  it('sets «dependencies unknown» apart from the state before it', () => {
    render(DepTree, {
      props: treeProps({ nodes: [leaf('x', { name: 'Xaero', deps_unknown: 'unreachable' })] }),
    });
    const mark = screen.getByText('dependencies unknown');
    const sep = mark.previousElementSibling;
    expect(sep?.textContent?.trim()).toBe('·');
    expect(sep?.getAttribute('aria-hidden')).toBe('true');
    // The description still reads the state and the mark, not the separator.
    expect(describedText(item('Xaero'))).toBe('installed dependencies unknown');
  });

  it('a disabled dependency jumps to its row too', async () => {
    const onJump = vi.fn();
    const x = leaf('px', { name: 'Xaero', installed: false, disabled: true });
    render(DepTree, { props: treeProps({ nodes: [x], onJump }) });
    await fireEvent.click(screen.getByRole('button', { name: 'Show Xaero in the list' }));
    expect(onJump).toHaveBeenCalledWith(x);
  });

  it('every row is one height, the height of its tallest control', () => {
    const nodes = [
      leaf('a', { children: [leaf('b')] }),
      leaf('arch', { name: 'Arch', installed: false }),
      leaf('d'),
    ];
    const { container } = render(DepTree, { props: treeProps({ nodes }) });
    // A (a chevron), its open child B, Arch, D — each with the three action columns.
    const rows = [...container.querySelectorAll('.tree-row')];
    expect(rows).toHaveLength(4);
    for (const row of rows) {
      expect(row.classList).toContain('min-h-7');
      expect(row.className).not.toMatch(/\bpy-/);
    }
  });
});

// Maintainer, 2026-10-07: every row carries the Installed row's actions — the switch, install or
// remove, and «show in the list» — drawn on every row and inactive where they cannot act, so the
// columns never have holes.
describe('DepTree — every row carries the list’s actions', () => {
  const ctxWith = (over: Partial<DepTreeCtx> = {}): DepTreeCtx => ({ ...EMPTY_TREE_CTX, ...over });

  it('an installed dependency: switch off, remove, show in the list', async () => {
    const onDisable = vi.fn();
    const onUninstall = vi.fn();
    const onJump = vi.fn();
    const x = leaf('px', { name: 'Xaero' });
    render(DepTree, {
      props: treeProps({ nodes: [x], onJump, ctx: ctxWith({ onDisable, onUninstall }) }),
    });
    const off = screen.getByRole('button', { name: 'Disable Xaero' });
    expect(off.classList).toContain('btn-icon-success');
    expect(off.querySelector('.lucide-power')).toBeTruthy();
    const remove = screen.getByRole('button', { name: 'Remove Xaero' });
    expect(remove.classList).toContain('btn-icon-danger');
    const locate = screen.getByRole('button', { name: 'Show Xaero in the list' });
    expect(locate.querySelector('.lucide-locate-fixed')).toBeTruthy();
    expect(locate.hasAttribute('disabled')).toBe(false);
    expect(locate.hasAttribute('aria-disabled')).toBe(false);
    await fireEvent.click(off);
    await fireEvent.click(remove);
    await fireEvent.click(locate);
    expect(onDisable).toHaveBeenCalledWith(x);
    expect(onUninstall).toHaveBeenCalledWith(x);
    expect(onJump).toHaveBeenCalledWith(x);
  });

  it('a switched-off dependency: switch on, remove, show in the list', async () => {
    const onEnable = vi.fn();
    const onUninstall = vi.fn();
    const x = leaf('px', { name: 'Xaero', installed: false, disabled: true });
    render(DepTree, { props: treeProps({ nodes: [x], ctx: ctxWith({ onEnable, onUninstall }) }) });
    const on = screen.getByRole('button', { name: 'Enable Xaero' });
    expect(on.classList).toContain('!text-muted');
    await fireEvent.click(on);
    expect(onEnable).toHaveBeenCalledWith(x);
    await fireEvent.click(screen.getByRole('button', { name: 'Remove Xaero' }));
    expect(onUninstall).toHaveBeenCalledWith(x);
    const locate = screen.getByRole('button', { name: 'Show Xaero in the list' });
    expect(locate.hasAttribute('disabled')).toBe(false);
    expect(locate.hasAttribute('aria-disabled')).toBe(false);
  });

  it('an absent dependency keeps the columns: its switch and «show» are there, inactive', () => {
    render(DepTree, {
      props: treeProps({ nodes: [leaf('arch', { name: 'Arch', installed: false })] }),
    });
    const toggle = screen.getByRole('button', { name: 'Enable Arch' });
    const locate = screen.getByRole('button', { name: 'Show Arch in the list' });
    for (const b of [toggle, locate]) {
      // Unavailable but reachable (spec 2026-10-08 D1): aria-disabled, its reason its own tooltip;
      // the wrapping span is only the column.
      expect(b.hasAttribute('disabled')).toBe(false);
      expect(b.getAttribute('aria-disabled')).toBe('true');
      expect(b.parentElement?.tagName).toBe('SPAN');
      expect(b.parentElement?.getAttribute('data-slot')).not.toBeNull();
    }
    expect(screen.queryByRole('button', { name: 'Remove Arch' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Install Arch' }).hasAttribute('disabled')).toBe(
      false,
    );
  });

  // Spec 2026-10-07 §8: «show in the list» is a column left of the name; the switch and install or
  // remove end the row, as the mod row's own switch and Remove end it — so the panel's columns
  // run on under the row's.
  it('every row has the three columns: «show» before the name, the list’s two at its end', () => {
    const nodes = [
      leaf('x', { name: 'Xaero' }),
      leaf('arch', { name: 'Arch', installed: false }),
      leaf('off', { name: 'Off', installed: false, disabled: true }),
    ];
    render(DepTree, { props: treeProps({ nodes }) });
    for (const name of ['Xaero', 'Arch', 'Off']) {
      const row = item(name).firstElementChild as HTMLElement;
      const slots = [...row.querySelectorAll('[data-slot]')];
      expect(slots.map((el) => el.getAttribute('data-slot'))).toEqual([
        'locate',
        'toggle',
        'presence',
      ]);
      const nameBtn = screen.getByRole('button', { name });
      const order = [...row.querySelectorAll('*')];
      const at = (el: Element | undefined) => order.indexOf(el as Element);
      expect(at(slots[0])).toBeLessThan(at(nameBtn));
      expect(at(nameBtn)).toBeLessThan(at(slots[1]));
      // The switch and install-or-remove close the row, a 4 px step apart like ModCard's.
      const actions = slots[1]?.parentElement as HTMLElement;
      expect(actions.lastElementChild).toBe(slots[2]);
      expect(actions.classList).toContain('gap-1');
    }
  });

  // Focus survives a removal (DESIGN.md §13): each column keeps ONE button whatever the node's
  // state, so the button the user pressed is still there — still focused — when the graph comes
  // back with the node absent or switched off.
  it('a column’s button is one element across the node’s states', async () => {
    const x = leaf('px', { name: 'Xaero' });
    const { rerender } = render(DepTree, { props: treeProps({ nodes: [x] }) });
    const remove = screen.getByRole('button', { name: 'Remove Xaero' });
    const toggle = screen.getByRole('button', { name: 'Disable Xaero' });
    await rerender(treeProps({ nodes: [{ ...x, installed: false, disabled: true }] }));
    expect(screen.getByRole('button', { name: 'Enable Xaero' })).toBe(toggle);
    expect(screen.getByRole('button', { name: 'Remove Xaero' })).toBe(remove);
    await rerender(treeProps({ nodes: [{ ...x, installed: false, disabled: false }] }));
    expect(screen.getByRole('button', { name: 'Install Xaero' })).toBe(remove);
  });

  it('an install in flight spins on its own node, and another click there does nothing', async () => {
    const onInstall = vi.fn();
    const nodes = [
      leaf('arch', { name: 'Arch', installed: false }),
      leaf('lib', { name: 'Lib', installed: false }),
    ];
    render(DepTree, {
      props: treeProps({
        nodes,
        onInstall,
        ctx: ctxWith({ installing: (k) => k === 'modrinth:arch' }),
      }),
    });
    const busy = screen.getByRole('button', { name: 'Install Arch' });
    expect(busy.getAttribute('aria-busy')).toBe('true');
    // aria-disabled, not disabled: a disabled button drops focus, and the node flips to
    // installed under it when the install lands.
    expect(busy.getAttribute('aria-disabled')).toBe('true');
    expect(busy.querySelector('[role="status"]')).toBeTruthy();
    await fireEvent.click(busy);
    expect(onInstall).not.toHaveBeenCalled();
    const idle = screen.getByRole('button', { name: 'Install Lib' });
    expect(idle.getAttribute('aria-busy')).not.toBe('true');
    await fireEvent.click(idle);
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  // The install's graph reload can land before the list has the new row: the node already says
  // «installed», but no jar is listed for its switch, Remove or «show» to act on. Until the install
  // is done the node stays busy and none of them acts.
  it('a node stays busy until its install is done, even once the graph says it is there', async () => {
    const onUninstall = vi.fn();
    const onDisable = vi.fn();
    const onJump = vi.fn();
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('arch', { name: 'Arch' })],
        onJump,
        ctx: ctxWith({ installing: () => true, onUninstall, onDisable }),
      }),
    });
    const presence = screen.getByRole('button', { name: 'Install Arch' });
    expect(presence.getAttribute('aria-busy')).toBe('true');
    await fireEvent.click(presence);
    const toggle = screen.getByRole('button', { name: 'Disable Arch' });
    const locate = screen.getByRole('button', { name: 'Show Arch in the list' });
    expect(toggle.getAttribute('aria-disabled')).toBe('true');
    expect(locate.getAttribute('aria-disabled')).toBe('true');
    expect(onUninstall).not.toHaveBeenCalled();
    expect(onDisable).not.toHaveBeenCalled();
    expect(onJump).not.toHaveBeenCalled();
  });
});

// Spec 2026-10-07 D1: pointing at a row fills it like a list row, and the row is the pointer's way
// to the mod's details — the name stays the keyboard's (Enter).
describe('DepTree — the row is the target', () => {
  const rowOf = (name: string) => item(name).firstElementChild as HTMLElement;

  it('a click on the row opens the mod’s details', async () => {
    const onOpenDetail = vi.fn();
    render(DepTree, { props: treeProps({ onOpenDetail }) });
    await fireEvent.click(rowOf('D'));
    expect(onOpenDetail.mock.calls).toEqual([['modrinth', 'd']]);
  });

  it('a click on one of its buttons is that button’s alone', async () => {
    const onOpenDetail = vi.fn();
    const onUninstall = vi.fn();
    render(DepTree, {
      props: treeProps({ onOpenDetail, ctx: { ...EMPTY_TREE_CTX, onUninstall } }),
    });
    await fireEvent.click(screen.getByRole('button', { name: 'D' }));
    expect(onOpenDetail).toHaveBeenCalledTimes(1);
    await fireEvent.click(screen.getByRole('button', { name: 'Remove D' }));
    expect(onUninstall).toHaveBeenCalledTimes(1);
    await fireEvent.click(item('A').querySelector('[data-tree-toggle]') as HTMLElement);
    expect(onOpenDetail).toHaveBeenCalledTimes(1);
  });

  it('a nested row opens its own mod, never its parents’', async () => {
    const onOpenDetail = vi.fn();
    render(DepTree, { props: treeProps({ onOpenDetail }) });
    await fireEvent.click(rowOf('B'));
    expect(onOpenDetail.mock.calls).toEqual([['modrinth', 'b']]);
  });

  it('the name reads as the row’s, not as a link', () => {
    render(DepTree, { props: treeProps() });
    expect(screen.getByRole('button', { name: 'D' }).classList).not.toContain('btn-tertiary');
    expect(rowOf('D').classList).toContain('hover:bg-subtle');
    expect(rowOf('D').classList).toContain('cursor-pointer');
  });
});

// Spec 2026-10-07 D5: «⟳ цикл» told the user nothing. The graph seeds every path with the panel's
// own mod (depgraph.rs), so an edge back to it is a cycle — said as «this mod»; any other repeat
// is «expanded above».
describe('DepTree — the cycle mark says what it is', () => {
  const selfUnder = (props: Record<string, unknown> = {}) =>
    treeProps({
      rootKey: 'modrinth:a',
      nodes: [leaf('b', { children: [leaf('a', { name: 'Alpha', cycle: true })] })],
      ...props,
    });

  it('the panel’s own mod, met again below, has «this mod» as its state', () => {
    render(DepTree, { props: selfUnder() });
    const self = item('Alpha');
    expect(self.querySelector('[id$="-state"]')?.textContent?.trim()).toBe('this mod');
    // No second element for the mark: the description names only what is in the DOM.
    expect(self.querySelector('[id$="-cycle"]')).toBeNull();
    expect(describedText(self)).toBe('this mod');
    expect(self.querySelector('.lucide-corner-left-up')).toBeTruthy();
    // Its actions are an installed node's — the same as the row the panel hangs under.
    expect(screen.getByRole('button', { name: 'Remove Alpha' })).toBeTruthy();
    const showAlpha = screen.getByRole('button', { name: 'Show Alpha in the list' });
    expect(showAlpha.hasAttribute('disabled')).toBe(false);
    expect(showAlpha.hasAttribute('aria-disabled')).toBe(false);
  });

  it('a version mismatch on that edge keeps its state and Fix; the mark follows it', () => {
    const conflict: DepViolation = {
      kind: 'version_out_of_range',
      dependent_name: 'B',
      dependent_sha1: 'b-sha',
      dep_id: 'a',
      needed: '>=2',
      needed_desc: rawRangeDesc('>=2'),
      installed_version: '1.0',
      provider_project: { source: 'modrinth', project_id: 'a', version_id: null },
      provider_sha1: null,
      family: null,
    };
    render(DepTree, {
      props: selfUnder({
        ctx: {
          ...EMPTY_TREE_CTX,
          report: { violations: [conflict] },
          enabledShaOf: () => 'b-sha',
          conflictOf: () => conflict,
        },
      }),
    });
    const self = item('Alpha');
    expect(self.getAttribute('data-node-state')).toBe('out_of_range');
    expect(describedText(self)).toBe('version mismatch this mod');
    expect(
      screen.getByRole('button', { name: 'Fix the version conflict with Alpha' }),
    ).toBeTruthy();
  });

  it('any other repeat keeps its state; «expanded above» follows it, set apart', () => {
    render(DepTree, {
      props: treeProps({
        rootKey: 'modrinth:root',
        nodes: [leaf('b', { children: [leaf('x', { name: 'Xaero', cycle: true })] })],
      }),
    });
    const x = item('Xaero');
    expect(describedText(x)).toBe('installed expanded above');
    const mark = x.querySelector('[id$="-cycle"]') as HTMLElement;
    expect(mark.textContent?.trim()).toBe('expanded above');
    expect(mark.previousElementSibling?.textContent?.trim()).toBe('·');
    expect(screen.queryByText('cycle')).toBeNull();
  });
});

// Spec 2026-10-07 §9: optional is drawn, not only said. Below the top level a node's children come
// required first, then optional, and each draws its own guide segment — solid or dashed.
describe('DepTree — an optional edge is drawn dashed', () => {
  const branch = () =>
    treeProps({
      nodes: [
        leaf('p', {
          name: 'Parent',
          children: [
            leaf('o1', { name: 'Opt One', installed: false, declared: 'optional' }),
            leaf('r1', { name: 'Req One' }),
            leaf('o2', { name: 'Opt Two', installed: false, declared: 'optional' }),
            leaf('r2', { name: 'Req Two' }),
          ],
        }),
      ],
    });

  it('lists a branch’s required children before its optional ones, each kept in its order', () => {
    render(DepTree, { props: branch() });
    const names = [...item('Parent').querySelectorAll('[role="group"] [data-tree-name]')].map((b) =>
      b.textContent?.trim(),
    );
    expect(names).toEqual(['Req One', 'Req Two', 'Opt One', 'Opt Two']);
  });

  it('draws an optional child’s guide dashed and a required one’s solid', () => {
    render(DepTree, { props: branch() });
    expect(item('Opt One').classList).toContain('border-dashed');
    expect(item('Opt One').classList).toContain('border-l');
    expect(item('Req One').classList).toContain('border-l');
    expect(item('Req One').classList).not.toContain('border-dashed');
    // The top level has no guide: its section's stripe heads it.
    expect(item('Parent').classList).not.toContain('border-l');
  });
});

// Spec 2026-10-07 §9: each section is a block with its own stripe and glyph — the relation cell's
// ⛓ for what the mod requires, its ↑ for what requires the mod, a dashed stripe for optional.
describe('DepSection — sections told apart at a glance', () => {
  const sections = () =>
    render(DepSection, {
      props: {
        root: {
          sha1: 'a',
          source: 'modrinth' as const,
          project_id: 'PA',
          name: 'Alpha',
          required: [leaf('PB', { name: 'Bravo' })],
          optional: [leaf('PO', { name: 'Oscar', installed: false, declared: 'optional' })],
        },
        requiredBy: [{ name: 'Gamma', source: 'modrinth' as const, projectId: 'PG', sha1: 'g' }],
        onInstall: () => {},
        onJump: () => {},
        onOpenDetail: () => {},
      },
    });
  const block = (kind: string) =>
    document.querySelector(`[data-dep-block="${kind}"]`) as HTMLElement;

  it('gives each section its own stripe', () => {
    sections();
    expect(block('requires').classList).toContain('border-accent');
    expect(block('optional').classList).toContain('border-dashed');
    expect(block('required-by').classList).toContain('border-relation-by');
  });

  it('heads «Requires» with the cell’s ⛓ and «Required by» with its ↑', () => {
    sections();
    expect(block('requires').querySelector('.lucide-link-2')).toBeTruthy();
    expect(block('required-by').querySelector('.lucide-arrow-up')).toBeTruthy();
    expect(block('optional').querySelector('svg.lucide-link-2, svg.lucide-arrow-up')).toBeNull();
    // The trees stay named by their headings.
    expect(block('requires').querySelector('[role="tree"]')?.getAttribute('aria-labelledby')).toBe(
      'dep-req-a',
    );
  });
});

// Spec 2026-10-08 dep-tree-keyboard-reasons: what the tree tells a pointer user reaches the keyboard.
// An action that cannot act stays in the item's Tab order (aria-disabled) and says why (D1); a mark
// is a definition — a button whose explanation shows on keyboard focus (D2).
describe('DepTree — reasons and marks reach the keyboard', () => {
  const ctxOf = (over: Partial<DepTreeCtx> = {}): DepTreeCtx => ({ ...EMPTY_TREE_CTX, ...over });
  // happy-dom models no focus modality: this is a Tab, as use:tooltip sees one. Bubbling, so a
  // tooltip left on a wrapper around the button would answer too.
  const keyboardFocus = (el: HTMLElement) => {
    el.matches = () => true;
    el.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
  };
  const absent = () => treeProps({ nodes: [leaf('arch', { name: 'Arch', installed: false })] });
  afterEach(() => hideTooltip());

  it('an absent mod’s switch and «show» stay reachable: aria-disabled, on the item’s tab stop', () => {
    render(DepTree, { props: absent() });
    for (const name of ['Enable Arch', 'Show Arch in the list']) {
      const b = screen.getByRole('button', { name });
      expect(b.hasAttribute('disabled')).toBe(false);
      expect(b.getAttribute('aria-disabled')).toBe('true');
      expect(b.getAttribute('tabindex')).toBe(item('Arch').getAttribute('tabindex'));
    }
  });

  it('keyboard focus on each says why it cannot act, and describes it with that', () => {
    render(DepTree, { props: absent() });
    const cases: [string, RegExp][] = [
      ['Show Arch in the list', /^Arch isn't installed, so it has no row in the list$/],
      ['Enable Arch', /^Arch isn't installed, so there is nothing to switch on$/],
    ];
    for (const [name, reason] of cases) {
      const b = screen.getByRole('button', { name });
      const before = tooltipState.shown;
      keyboardFocus(b);
      expect(tooltipState.text).toMatch(reason);
      expect(b.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
      // One showing: the button's own — no wrapper around it answered the bubbling focus too.
      expect(tooltipState.shown).toBe(before + 1);
      hideTooltip();
    }
  });

  // D1b through Svelte: the switch holds focus with its reason up when the mod arrives — the
  // tooltip follows the button to its action and stops describing it with the old reason.
  it('a focused switch whose mod arrives says its action, no longer the reason', async () => {
    const { rerender } = render(DepTree, { props: absent() });
    const sw = screen.getByRole('button', { name: 'Enable Arch' });
    keyboardFocus(sw);
    expect(tooltipState.text).toMatch(/isn't installed/);
    await rerender(treeProps({ nodes: [leaf('arch', { name: 'Arch', installed: true })] }));
    expect(sw.getAttribute('aria-label')).toBe('Disable Arch');
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Disable Arch');
    expect(sw.hasAttribute('aria-describedby')).toBe(false);
  });

  it('pressing them does nothing', async () => {
    const onJump = vi.fn();
    const onEnable = vi.fn();
    const onDisable = vi.fn();
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('arch', { name: 'Arch', installed: false })],
        onJump,
        ctx: ctxOf({ onEnable, onDisable }),
      }),
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Enable Arch' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Show Arch in the list' }));
    expect(onJump).not.toHaveBeenCalled();
    expect(onEnable).not.toHaveBeenCalled();
    expect(onDisable).not.toHaveBeenCalled();
  });

  it('while its install runs, a mod with a row keeps them inert and named by their action', async () => {
    const onJump = vi.fn();
    const onDisable = vi.fn();
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('arch', { name: 'Arch' })],
        onJump,
        ctx: ctxOf({ installing: () => true, onDisable }),
      }),
    });
    for (const name of ['Disable Arch', 'Show Arch in the list']) {
      const b = screen.getByRole('button', { name });
      expect(b.getAttribute('aria-disabled')).toBe('true');
      keyboardFocus(b);
      expect(tooltipState.text).toBe(name);
      hideTooltip();
      await fireEvent.click(b);
    }
    expect(onDisable).not.toHaveBeenCalled();
    expect(onJump).not.toHaveBeenCalled();
  });

  it('«dependencies unknown» is a definition: on the tab stop, explained on focus, pressing it opens nothing', async () => {
    const onOpenDetail = vi.fn();
    render(DepTree, {
      props: treeProps({
        nodes: [leaf('x', { name: 'Xaero', deps_unknown: 'unreachable' })],
        onOpenDetail,
      }),
    });
    const mark = screen.getByRole('button', { name: 'dependencies unknown' });
    expect(mark.id).toMatch(/-unknown$/);
    expect(mark.getAttribute('tabindex')).toBe(item('Xaero').getAttribute('tabindex'));
    keyboardFocus(mark);
    expect(tooltipState.text).toMatch(/the platform is unavailable/i);
    expect(mark.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
    await fireEvent.click(mark);
    expect(onOpenDetail).not.toHaveBeenCalled();
  });

  it('«this mod», as the self node’s state, is a definition', async () => {
    const onOpenDetail = vi.fn();
    render(DepTree, {
      props: treeProps({
        rootKey: 'modrinth:a',
        nodes: [leaf('b', { children: [leaf('a', { name: 'Alpha', cycle: true })] })],
        onOpenDetail,
      }),
    });
    const self = screen.getByRole('button', { name: 'this mod' });
    expect(self.id).toMatch(/-state$/);
    expect(self.getAttribute('tabindex')).toBe(item('Alpha').getAttribute('tabindex'));
    keyboardFocus(self);
    expect(tooltipState.text).toBe('This is Alpha itself — its dependencies are listed above.');
    expect(self.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
    await fireEvent.click(self);
    expect(onOpenDetail).not.toHaveBeenCalled();
  });

  it('«expanded above» is a definition', async () => {
    const onOpenDetail = vi.fn();
    render(DepTree, {
      props: treeProps({
        rootKey: 'modrinth:root',
        nodes: [leaf('b', { children: [leaf('x', { name: 'Xaero', cycle: true })] })],
        onOpenDetail,
      }),
    });
    const mark = screen.getByRole('button', { name: 'expanded above' });
    expect(mark.id).toMatch(/-cycle$/);
    expect(mark.getAttribute('tabindex')).toBe(item('Xaero').getAttribute('tabindex'));
    keyboardFocus(mark);
    expect(tooltipState.text).toMatch(/^Xaero is already expanded higher in this branch/);
    expect(mark.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
    await fireEvent.click(mark);
    expect(onOpenDetail).not.toHaveBeenCalled();
  });
});
