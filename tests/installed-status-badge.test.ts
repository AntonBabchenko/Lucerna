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
  hoveredKey: null,
  updateState: null,
  checking: false,
  packChip: null,
  selected: false,
  onToggleExpand() {},
  onHover() {},
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

  it('sits between the badges and the actions inside the list row', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        depTotal: 1,
        updateState: { kind: 'update_available', target: { version_number: '2.0' } } as never,
      },
    });
    const row = screen.getByTestId('card-list-row');
    const badge = within(row).getByTestId('mod-update-badge');
    const p = within(row).getByTestId('relation-pill');
    const update = within(row).getByRole('button', { name: 'Update' });
    expect(badge.compareDocumentPosition(p) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(p.compareDocumentPosition(update) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
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
