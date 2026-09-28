import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import InstalledModRow from '$lib/mods/installed/InstalledModRow.svelte';
import type { RowProblem } from '$lib/mods/installed/row-problem';

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

describe('dependency relation chip', () => {
  const depChips = () =>
    screen.getAllByRole('button').filter((b) => /dep|required by/i.test(b.textContent ?? ''));

  // Loader-scoping a merged multi-loader jar's foreign-family children can empty
  // `required` entirely. The chip is the ONLY control that opens DepSection, so
  // gating it on required-deps alone would hide the still-correct optional
  // section as the price of hiding a phantom one.
  it('renders the chip for an optional-only row, with a non-empty label', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        depTotal: 0,
        requiredBy: [],
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
    const chip = screen.getByTestId('dep-expand-chip');
    // Assert WHICH label fills the slot, not merely that one exists: a widened
    // gate with no label part renders a bare chevron, and a wrong/stale i18n key
    // would still satisfy a non-empty check.
    expect(chip.textContent).toMatch(/optional/i);
  });

  it('renders a SINGLE toggle combining both counts when the mod has deps AND is required-by', async () => {
    const onToggleExpand = vi.fn();
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        depTotal: 1,
        requiredBy: [{ name: 'Beta', source: 'modrinth', projectId: 'pb', sha1: 'b' }],
        onToggleExpand,
      },
    });

    const chips = depChips();
    expect(chips).toHaveLength(1);
    expect(chips[0].textContent).toMatch(/1 dep/i);
    expect(chips[0].textContent).toMatch(/required by 1/i);

    await fireEvent.click(chips[0]);
    expect(onToggleExpand).toHaveBeenCalledTimes(1);
  });

  it('renders the single chip with just the dep count when the mod is not required-by', () => {
    render(InstalledModRow, {
      props: { ...base(), installed: installed(true), depTotal: 2, requiredBy: [] },
    });
    const chips = depChips();
    expect(chips).toHaveLength(1);
    expect(chips[0].textContent).toMatch(/2 deps/i);
    expect(chips[0].textContent).not.toMatch(/required by/i);
  });

  it('renders the single chip with just required-by when the mod has no deps of its own', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        depTotal: 0,
        requiredBy: [{ name: 'Beta', source: 'modrinth', projectId: 'pb', sha1: 'b' }],
      },
    });
    const chips = depChips();
    expect(chips).toHaveLength(1);
    expect(chips[0].textContent).toMatch(/required by 1/i);
    expect(chips[0].textContent).not.toMatch(/\bdep\b/i);
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
