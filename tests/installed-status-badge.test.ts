import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import InstalledModRow from '$lib/mods/installed/InstalledModRow.svelte';
import type { RowProblem } from '$lib/mods/installed/row-problem';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

const summary = {
  source: 'modrinth' as const,
  project_id: 'p',
  slug: 's',
  name: 'Alpha',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: 'x',
  updated_at: null,
};
const installed = (enabled: boolean) => ({
  filename: 'a.jar',
  sha1: 'a',
  source: 'modrinth' as const,
  project_id: 'p',
  version_id: 'v',
  name: 'Alpha',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled,
  enrich_attempted: false,
});
const base = () => ({
  summary,
  rowKey: 'modrinth:p',
  root: undefined,
  requiredBy: [],
  depTotal: 0,
  problem: null,
  expanded: false,
  graphLoading: false,
  updateState: null,
  checking: false,
  packChip: null,
  selected: false,
  onToggleExpand() {},
  onOpenDetail() {},
  onOpenDetailMod() {},
  onToggle() {},
  onUninstall() {},
  onUpdate() {},
  onShowChangelog() {},
  onSelectChange() {},
  onInstallDep() {},
  onJump() {},
  onProblemFix() {},
  onRevealProblems() {},
});

const blocking = (over: Partial<RowProblem> = {}): RowProblem => ({
  level: 'blocking',
  text: 'Alpha needs Balm, which is not installed',
  tooltip: null,
  more: 0,
  fix: null,
  ...over,
});

describe('status badge priority', () => {
  // The danger badge that used to live here counted the GRAPH's absent required
  // children — the platform's claim, which a measured mod's own jar contradicts.
  // A real problem is a pre-flight violation, and it is marked by the ModCard's
  // danger accent plus PreflightPanel above the list, not by a left-side badge.
  it('shows NO left-side badge even when the pre-flight flags the row', () => {
    render(InstalledModRow, {
      props: { ...base(), installed: installed(false), problem: blocking() },
    });
    expect(screen.queryByTestId('status-badge')).toBeNull();
  });
  it('shows NO left-side badge for an update-available row (the ModCard shows vOld → vNew + Update on the right)', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        updateState: { kind: 'update_available', target: { version_number: '2.0' } } as never,
      },
    });
    expect(screen.queryByTestId('status-badge')).toBeNull();
  });
  it('shows NO badge when disabled (the ModCard shows the enable/disable state on the right)', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(false) } });
    expect(screen.queryByTestId('status-badge')).toBeNull();
  });
  it('shows no badge when enabled, no missing deps', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true) } });
    expect(screen.queryByTestId('status-badge')).toBeNull();
  });
});

describe('dependency relation pill', () => {
  const pill = () => screen.getByTestId('relation-pill');
  const requiredBy = [{ name: 'Beta', source: 'modrinth' as const, projectId: 'pb', sha1: 'b' }];

  // Loader-scoping can empty `required`; the pill is then the ONLY opener of the still-correct
  // optional section, so it must render and say what it counts.
  it('renders for an optional-only row, named by the optional count', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        root: {
          sha1: 'a',
          source: 'modrinth',
          project_id: 'p',
          name: 'Alpha',
          required: [],
          optional: [{ source: 'modrinth', project_id: 'sod', name: 'Sodium' }],
        } as never,
      },
    });
    expect(pill().getAttribute('aria-label')).toMatch(/1 optional/i);
  });

  it('is ONE control naming both directions, and it toggles the panel', async () => {
    const onToggleExpand = vi.fn();
    render(InstalledModRow, {
      props: { ...base(), installed: installed(true), depTotal: 1, requiredBy, onToggleExpand },
    });
    expect(screen.getAllByTestId('relation-pill')).toHaveLength(1);
    expect(pill().getAttribute('aria-label')).toMatch(/1 dep/i);
    expect(pill().getAttribute('aria-label')).toMatch(/required by 1/i);
    expect(pill().textContent?.replace(/\s+/g, '')).toBe('11');
    await fireEvent.click(pill());
    expect(onToggleExpand).toHaveBeenCalledTimes(1);
  });

  it('counts only own dependencies when nothing requires the mod', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true), depTotal: 2 } });
    expect(pill().getAttribute('aria-label')).toMatch(/2 deps/i);
    expect(pill().getAttribute('aria-label')).not.toMatch(/required by/i);
  });

  it('counts only dependents when the mod needs nothing', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true), requiredBy } });
    expect(pill().getAttribute('aria-label')).toMatch(/required by 1/i);
    expect(pill().getAttribute('aria-label')).not.toMatch(/\bdep\b/i);
  });

  // What the platform could not describe is not "needs nothing" (fallback discipline: "could not
  // tell" ≠ "absent"): the count is a question mark, the name says so, the tooltip says why.
  it('shows a mod whose dependencies are unknown as unknown — never as zero — and why', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        requiredBy,
        root: {
          sha1: 'a',
          source: 'modrinth',
          project_id: 'p',
          name: 'Alpha',
          required: [],
          optional: [],
          deps_unknown: 'unreachable',
        } as never,
      },
    });
    expect(pill().getAttribute('aria-label')).toMatch(/^dependencies unknown · required by 1$/i);
    expect(pill().textContent?.replace(/\s+/g, '')).toBe('?1');
    revealTooltip(pill());
    expect(tooltipState.text).toMatch(/the platform is unavailable/i);
    expect(tooltipState.text).toMatch(/required by 1/i);
    hideTooltip();
  });

  // A platform mod whose details did not load still has its dependencies: the pill opens them.
  it('a row whose details did not load still offers and opens its dependencies', async () => {
    const onToggleExpand = vi.fn();
    const root = {
      sha1: 'a',
      source: 'modrinth',
      project_id: 'p',
      name: 'Alpha',
      required: [
        {
          source: 'modrinth',
          project_id: 'lib',
          name: 'Lib',
          installed: true,
          declared: 'required',
          cycle: false,
          children: [],
        },
      ],
      optional: [],
    } as never;
    const { rerender } = render(InstalledModRow, {
      props: {
        ...base(),
        summary: null,
        installed: installed(true),
        depTotal: 1,
        root,
        onToggleExpand,
      },
    });
    await fireEvent.click(
      within(screen.getByTestId('manual-mod-row')).getByTestId('relation-pill'),
    );
    expect(onToggleExpand).toHaveBeenCalledOnce();
    await rerender({
      ...base(),
      summary: null,
      installed: installed(true),
      depTotal: 1,
      root,
      expanded: true,
    });
    expect(screen.getByRole('tree')).toBeTruthy();
  });
});

describe('second line', () => {
  it('a healthy row has none — no changelog chip, no relation chip', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        depTotal: 1,
        updateState: { kind: 'update_available', target: { version_number: '2.0' } } as never,
      },
    });
    expect(screen.queryByTestId('row-problem')).toBeNull();
    expect(screen.queryByTestId('mod-changelog-btn')).toBeNull();
    expect(screen.queryByTestId('dep-expand-chip')).toBeNull();
  });
});

describe('version node', () => {
  it("tooltips the version with the jar's file name", () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true) } });
    revealTooltip(screen.getByTestId('mod-version'));
    expect(tooltipState.text).toBe('a.jar');
    hideTooltip();
  });

  it('keeps the held pin beside the version it keeps', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true), held: true } });
    const version = screen.getByTestId('mod-version');
    expect(version.parentElement?.contains(screen.getByTestId('mod-held-pin'))).toBe(true);
  });
});

// The compat badge is folded into the reason line (spec §6.2): one short reason, one fix, and the
// colour of its level — red for what stops the game, amber for what may not work.
describe('problem line', () => {
  beforeAll(() => locale.set('en'));

  // Guard (green before and after); the four below are the RED ones.
  it('renders no second line for a row without a problem', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true) } });
    expect(screen.queryByTestId('row-problem')).toBeNull();
    expect(screen.queryByTestId('incompat-badge')).toBeNull();
  });

  it('a blocking reason is red, with a red accent strip', () => {
    const { container } = render(InstalledModRow, {
      props: { ...base(), installed: installed(true), problem: blocking() },
    });
    const line = screen.getByTestId('row-problem');
    expect(line.textContent).toContain('Alpha needs Balm, which is not installed');
    expect(line.querySelector('.text-danger')).not.toBeNull();
    expect(container.querySelector('[data-card-accent]')?.className).toContain('bg-danger');
  });

  it('a warning is amber, with an amber accent strip', () => {
    const { container } = render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        problem: blocking({
          level: 'warning',
          text: 'May not work: no release for NeoForge 1.21.1',
        }),
      },
    });
    const line = screen.getByTestId('row-problem');
    expect(line.querySelector('.text-warning-text')).not.toBeNull();
    expect(line.querySelector('.text-danger')).toBeNull();
    expect(container.querySelector('[data-card-accent]')?.className).toContain('bg-warning-text');
  });

  // Plan §5b V2 (screenshots 01, 06d): the line sat under the card, on the page background —
  // outside the card's surface and its accent strip. It is the card's own second line now, so the
  // strip and the hover fill cover both lines, and the row «show in the list» scrolls to (`[data-mod-row]`) holds
  // both.
  it('sits inside the card, under the row, with the strip and the jump target around both', () => {
    render(InstalledModRow, {
      props: { ...base(), installed: installed(true), problem: blocking() },
    });
    const line = screen.getByTestId('row-problem');
    const card = line.closest('[data-card-shell]');
    expect(card).toBe(screen.getByTestId('card-list-row'));
    expect(card?.querySelector('[data-card-accent]')?.className).toContain('bg-danger');
    expect(card?.closest('[data-mod-row]')).not.toBeNull();
  });

  it('a manual jar’s line sits inside its card too', () => {
    const manual = { ...installed(true), source: null, project_id: null, version_id: null };
    render(InstalledModRow, {
      props: { ...base(), summary: null, installed: manual as never, problem: blocking() },
    });
    const line = screen.getByTestId('row-problem');
    expect(line.closest('[data-card-shell]')).toBe(screen.getByTestId('manual-mod-row'));
  });

  it('offers exactly one fix and reports which', async () => {
    const onProblemFix = vi.fn();
    const fix = { kind: 'choose_version' as const, label: 'Choose version' };
    render(InstalledModRow, {
      props: { ...base(), installed: installed(true), problem: blocking({ fix }), onProblemFix },
    });
    const buttons = within(screen.getByTestId('row-problem')).getAllByRole('button');
    expect(buttons.map((b) => b.textContent?.trim())).toEqual(['Choose version']);
    await fireEvent.click(within(screen.getByTestId('row-problem')).getByRole('button'));
    expect(onProblemFix).toHaveBeenCalledWith(fix);
  });

  // Plan §5c V3 (screenshot n01b): at 820 px the line wrapped as a row of flex items — its ✕ alone
  // on the first line, the reason on the second, «and 1 more · Fix…» on a third. The icon keeps to
  // the reason's first line, the reason wraps under itself, and «and N more · Fix…» follows it
  // inline — one unit that never breaks — or takes one line of its own.
  it("keeps the icon on the reason's first line and the fix after the reason, as one unit", () => {
    const fix = { kind: 'choose_version' as const, label: 'Choose version' };
    render(InstalledModRow, {
      props: { ...base(), installed: installed(true), problem: blocking({ more: 1, fix }) },
    });
    const line = screen.getByTestId('row-problem');
    // Two columns, lined up by baseline: the icon's line sits on the reason's first line.
    expect(line.className).toMatch(/\bitems-baseline\b/);
    const [icon, reason] = [...line.children];
    expect(icon?.querySelector('svg')).not.toBeNull();
    expect(reason?.textContent).toContain('Alpha needs Balm, which is not installed');
    // The reason is text that wraps under itself — not a flex row that breaks between its parts.
    expect(reason?.className.split(/\s+/)).not.toContain('flex');
    // «and 1 more · Choose version» rides after it as one inline unit.
    const tail = within(reason as HTMLElement).getByTestId('row-problem-actions');
    expect(tail.className).toMatch(/\binline-flex\b/);
    expect(tail.className).toMatch(/\bwhitespace-nowrap\b/);
    expect(tail.contains(screen.getByTestId('row-problem-more'))).toBe(true);
    expect(tail.contains(screen.getByRole('button', { name: 'Choose version' }))).toBe(true);
  });

  it('folds further reasons into «and N more», which reveals the panel', async () => {
    const onRevealProblems = vi.fn();
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        problem: blocking({ more: 2 }),
        onRevealProblems,
      },
    });
    const more = screen.getByTestId('row-problem-more');
    expect(more.textContent).toContain('and 2 more');
    await fireEvent.click(more);
    expect(onRevealProblems).toHaveBeenCalledOnce();
  });
});

// The row's context menu (spec D13, §6.8): the file on disk, the project's page and the hold, all
// reachable by keyboard (Shift+F10 / the menu key) — the removal stays last (DESIGN.md §8).
describe('installed row menu', () => {
  beforeAll(() => locale.set('en'));
  const items = () => screen.getAllByRole('menuitem').map((m) => m.textContent?.trim());

  it('offers page, folder and hold for a platform mod; removal stays last', async () => {
    const onRevealFile = vi.fn();
    const onOpenProjectPage = vi.fn();
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        onRevealFile,
        onOpenProjectPage,
        hold: { held: false, onToggle: vi.fn() },
      },
    });
    await fireEvent.contextMenu(screen.getByTestId('card-list-row'));
    expect(items()).toEqual([
      'Disable',
      'Details',
      'Open mod page',
      'Show in folder',
      "Don't update",
      'Remove',
    ]);
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Show in folder' }));
    expect(onRevealFile).toHaveBeenCalledOnce();
    await fireEvent.contextMenu(screen.getByTestId('card-list-row'));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Open mod page' }));
    expect(onOpenProjectPage).toHaveBeenCalledOnce();
  });

  it('a held mod offers to allow updates again', async () => {
    const onToggle = vi.fn();
    render(InstalledModRow, {
      props: { ...base(), installed: installed(true), hold: { held: true, onToggle } },
    });
    await fireEvent.contextMenu(screen.getByTestId('card-list-row'));
    expect(items()).not.toContain("Don't update");
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Allow updates' }));
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it('a manual jar offers its folder but neither a page nor a hold', async () => {
    const manual = { ...installed(true), source: null, project_id: null, version_id: null };
    render(InstalledModRow, {
      props: {
        ...base(),
        summary: null,
        installed: manual as never,
        onRevealFile: vi.fn(),
        onOpenProjectPage: null,
        hold: null,
      },
    });
    await fireEvent.contextMenu(screen.getByTestId('manual-mod-row'));
    expect(items()).toContain('Show in folder');
    expect(items()).not.toContain('Open mod page');
    expect(items()).not.toContain("Don't update");
    expect(items().at(-1)).toBe('Remove');
  });
});
