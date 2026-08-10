import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledModRow from '$lib/mods/installed/InstalledModRow.svelte';

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
  hasPreflightIssue: false,
  expanded: false,
  graphLoading: false,
  hoveredKey: null,
  updateState: null,
  checking: false,
  packChip: null,
  incompatibleTitle: null,
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
});

describe('status badge priority', () => {
  // The danger badge that used to live here counted the GRAPH's absent required
  // children — the platform's claim, which a measured mod's own jar contradicts.
  // A real problem is a pre-flight violation, and it is marked by the ModCard's
  // danger accent plus PreflightPanel above the list, not by a left-side badge.
  it('shows NO left-side badge even when the pre-flight flags the row', () => {
    render(InstalledModRow, {
      props: { ...base(), installed: installed(false), hasPreflightIssue: true },
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
