/**
 * Tests for PreflightPanel: a long violation list must be wrapped in a
 * scrollable container so it cannot push the panel (or, when reused inside
 * the launch gate, the dialog's footer buttons) past the window edge.
 *
 * i18n resolves to real EN strings in the test environment.
 */
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
import type { PlanState } from '$lib/mods/violation-view';
import { describedText } from './test-utils/aria';
import { rangeDesc, rawRangeDesc } from './test-utils/range-desc';

// The panel names a dependency by one rule (`depDisplayName`): the provider's name from the
// report, else the app-wide name store — keyed by instance + (dependent, dep id); here by
// `dependent:dep` alone — else the raw id. The same on the Installed tab and at the Play gate.
const storeNames = vi.hoisted(() => new Map<string, string>());
vi.mock('$lib/mods/dep-names.svelte', () => ({
  depNameOf: (_instance: string, dependentSha1: string, depId: string) =>
    storeNames.get(`${dependentSha1}:${depId}`) ?? null,
  depProjectOf: () => null,
}));

function missing(i: number): DepViolation {
  return {
    kind: 'missing_required',
    dependent_name: `Mod ${i}`,
    dependent_sha1: `sha${i}`,
    dep_id: `dep-${i}`,
    needed: '',
    needed_desc: rawRangeDesc(''),
    installed_version: null,
    provider_project: null,
    provider_sha1: null,
    family: null,
  };
}

/** An actionable out-of-range violation (provider linked, family known). */
function outOfRange(): DepViolation {
  return {
    kind: 'version_out_of_range',
    dependent_name: 'indium',
    dependent_sha1: 'dep-sha',
    dep_id: 'sodium',
    needed: '0.5.11',
    needed_desc: rawRangeDesc('0.5.11'),
    installed_version: '0.9.0-beta.1',
    provider_project: { source: 'modrinth', project_id: 'AANobbMI', version_id: null },
    provider_sha1: 'old-sha',
    family: 'fabric_predicate',
  };
}

function reportWith(count: number): PreflightReport {
  return { violations: Array.from({ length: count }, (_, i) => missing(i)) };
}

describe('PreflightPanel violation wording', () => {
  /** The real AsyncParticles row: `type="incompatible"` with `(,6.0.9]`. */
  function incompatible(): DepViolation {
    return {
      kind: 'incompatible_installed',
      dependent_name: 'AsyncParticles',
      dependent_sha1: 'ap-sha',
      dep_id: 'create',
      needed: '(,6.0.9]',
      needed_desc: rangeDesc('(,6.0.9]', [{ kind: 'at_most', version: '6.0.9' }]),
      installed_version: '6.0.5',
      provider_project: { source: 'modrinth', project_id: 'create-id', version_id: null },
      provider_sha1: 'create-sha',
      family: 'maven',
    };
  }

  it('says "incompatible with", not "needs", and never shows Maven brackets', () => {
    const { getByTestId } = render(PreflightPanel, {
      props: { report: { violations: [incompatible()] } },
    });
    const row = getByTestId('preflight-row').textContent ?? '';
    expect(row).toContain('is incompatible with create 6.0.9 or older');
    expect(row).toContain('installed: 6.0.5');
    expect(row).not.toContain('(,6.0.9]');
  });

  it('offers no «Choose version» for an incompatibility', () => {
    // The picker marks the builds that satisfy the declared range as fitting —
    // for an incompatibility those are exactly the builds that clash. Its fix is
    // the planner's «Fix…», which judges the negation (the planner block below).
    const { queryByText } = render(PreflightPanel, {
      props: { report: { violations: [incompatible()] } },
    });
    expect(queryByText('Choose version')).toBeNull();
  });

  it('drops the range when an incompatibility constrains nothing', () => {
    // `type="incompatible"` with no versionRange is the natural way to say
    // "I break with this mod, period". Splicing the range in would produce
    // "incompatible with create any version" — or, for a bare spec,
    // "incompatible with create 1.2 recommended (any version works)", which
    // contradicts the very row it appears in.
    const v: DepViolation = {
      ...incompatible(),
      needed: '',
      needed_desc: rangeDesc('', [{ kind: 'any' }]),
      installed_version: '6.0.10',
    };
    const { getByTestId } = render(PreflightPanel, {
      props: { report: { violations: [v] } },
    });
    const row = getByTestId('preflight-row').textContent ?? '';
    expect(row).toContain('AsyncParticles is incompatible with create — installed: 6.0.10');
    expect(row).not.toContain('any version');
  });

  it('never says "any version works" inside an incompatibility', () => {
    // A bare (unbracketed) Maven spec on an `incompatible` dep: the range
    // accepts everything, so the incompatibility always fires. Rendering the
    // soft phrasing would tell the player any version works in the same
    // sentence that refuses to launch.
    const bare: DepViolation = {
      ...incompatible(),
      needed: '6.0.9',
      needed_desc: rangeDesc('6.0.9', [{ kind: 'soft', version: '6.0.9' }]),
    };
    const { getByTestId } = render(PreflightPanel, {
      props: { report: { violations: [bare] } },
    });
    expect(getByTestId('preflight-row').textContent).not.toContain('any version works');
  });

  // Not a requirement — the mod loads without it — but the installed build stops the load: a
  // blocker, never a fact about what the mod supports (plan §5b V1).
  it('words an out-of-range optional dependency as a blocker the mod can do without', () => {
    const v: DepViolation = {
      ...incompatible(),
      kind: 'optional_out_of_range',
      dep_id: 'curios',
      needed: '[9.0,)',
      needed_desc: rangeDesc('[9.0,)', [{ kind: 'at_least', version: '9.0' }]),
      installed_version: '5.4.0',
    };
    const { getByTestId } = render(PreflightPanel, {
      props: { report: { violations: [v] } },
    });
    expect(getByTestId('preflight-row').textContent).toContain(
      "AsyncParticles won't load with curios 5.4.0 — only with version 9.0 or newer, or without curios",
    );
  });
});

describe('PreflightPanel', () => {
  it('renders nothing when there are no violations', () => {
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] } },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
  });

  it('wraps the rows in a scrollable container', () => {
    const { getByTestId } = render(PreflightPanel, {
      props: { report: reportWith(40) },
    });
    const scroll = getByTestId('preflight-scroll');
    // The scroll container caps height and scrolls overflow.
    expect(scroll.className).toContain('overflow-y-auto');
    expect(scroll.className).toContain('max-h-');
    // Keyboard users must be able to focus and scroll the region.
    expect(scroll.getAttribute('tabindex')).toBe('0');
    expect(scroll.getAttribute('role')).toBe('region');
  });

  it('keeps every violation row inside the scroll container', () => {
    const { getByTestId, getAllByTestId } = render(PreflightPanel, {
      props: { report: reportWith(40) },
    });
    const scroll = getByTestId('preflight-scroll');
    const rows = getAllByTestId('preflight-row');
    expect(rows).toHaveLength(40);
    for (const row of rows) {
      expect(scroll.contains(row)).toBe(true);
    }
  });

  it('renders an install button on missing_required rows and calls onInstallMissing', async () => {
    const onInstallMissing = vi.fn();
    const report: PreflightReport = {
      violations: [
        {
          dependent_sha1: 'a',
          dependent_name: 'Waystones',
          dep_id: 'balm',
          kind: 'missing_required',
          installed_version: null,
          needed: '',
          needed_desc: rawRangeDesc(''),
          provider_project: null,
          provider_sha1: null,
          family: null,
        },
      ],
    };
    render(PreflightPanel, { props: { report, onInstallMissing } });
    const btn = screen.getByRole('button', { name: /balm/i });
    await fireEvent.click(btn);
    expect(onInstallMissing).toHaveBeenCalledWith(report.violations[0]);
  });

  it('offers Fix… and Choose version on a version conflict — never a blind Update', async () => {
    const onPlan = vi.fn();
    const v = outOfRange();
    render(PreflightPanel, { props: { report: { violations: [v] }, onPlan } });
    expect(screen.queryByText('Update')).toBeNull();
    expect(screen.getByText('Choose version')).toBeTruthy();
    await fireEvent.click(screen.getByText('Fix…'));
    expect(onPlan).toHaveBeenCalledWith(v);
  });

  it('calls onChooseVersion when Choose-version is clicked', async () => {
    const onChooseVersion = vi.fn();
    const v = outOfRange();
    const report: PreflightReport = { violations: [v] };
    render(PreflightPanel, { props: { report, onChooseVersion } });
    await fireEvent.click(screen.getByText('Choose version'));
    expect(onChooseVersion).toHaveBeenCalledWith(v);
  });

  it('shows the dead-end actions (no Fix…) when the row is in deadEndKeys', () => {
    const v = outOfRange();
    const report: PreflightReport = { violations: [v] };
    const { getByText, queryByText } = render(PreflightPanel, {
      props: {
        report,
        deadEndKeys: new Set([`${v.dependent_sha1}:${v.dep_id}`]),
      },
    });
    expect(queryByText('Fix…')).toBeNull();
    expect(getByText('No compatible version')).toBeTruthy();
    expect(getByText('Open mod page')).toBeTruthy();
    expect(getByText('Find alternative')).toBeTruthy();
  });

  it('renders a busy spinner (no action buttons) when the row is in busyKeys', () => {
    const v = outOfRange();
    const report: PreflightReport = { violations: [v] };
    const { queryByText, getByRole } = render(PreflightPanel, {
      props: {
        report,
        busyKeys: new Set([`${v.dependent_sha1}:${v.dep_id}`]),
      },
    });
    expect(queryByText('Fix…')).toBeNull();
    expect(queryByText('Choose version')).toBeNull();
    expect(getByRole('status')).toBeTruthy();
  });

  it('hides all per-row actions when showRowActions is false (launch-gate mode)', () => {
    const report: PreflightReport = { violations: [outOfRange(), missing(0)] };
    const { queryByText, queryAllByRole } = render(PreflightPanel, {
      props: { report, showRowActions: false },
    });
    expect(queryByText('Fix…')).toBeNull();
    expect(queryByText('Choose version')).toBeNull();
    expect(queryAllByRole('button')).toHaveLength(0);
  });
});

describe('PreflightPanel bulk migrate entry', () => {
  // Its own counted key: `mods.migration.openBtn` ("Fix incompatible mods") is shared with the
  // standalone migration flow and keeps its wording (spec §6.9).
  it('renders its own counted migrate button and calls onMigrate when count > 0', async () => {
    const onMigrate = vi.fn();
    render(PreflightPanel, {
      props: { report: reportWith(3), onMigrate, migrateCount: 3 },
    });
    const btn = screen.getByTestId('preflight-migrate-btn');
    expect(btn.textContent).toContain('Fix incompatible (3)');
    await fireEvent.click(btn);
    expect(onMigrate).toHaveBeenCalledTimes(1);
  });

  it('shows the panel (header + button, no rows) when there are incompatibilities but no dep violations', () => {
    // compat.incompatibleCount is authoritative and can be positive while the
    // dependency-graph report is empty (a jar's MC range read off metadata, an
    // MC dep declared as a recommendation). The bulk entry must still appear.
    const onMigrate = vi.fn();
    const { getByTestId, queryAllByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onMigrate, migrateCount: 2 },
    });
    expect(getByTestId('preflight-panel')).toBeTruthy();
    expect(getByTestId('preflight-migrate-btn')).toBeTruthy();
    expect(queryAllByTestId('preflight-row')).toHaveLength(0);
  });

  it('renders nothing when there are no violations and no incompatibilities', () => {
    const onMigrate = vi.fn();
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onMigrate, migrateCount: 0 },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
    expect(queryByTestId('preflight-migrate-btn')).toBeNull();
  });

  it('shows no migrate button in launch-gate shape (no onMigrate) even with violations', () => {
    // The launch gate reuses PreflightPanel without the migration props; its
    // remediation is its own "Fix and launch" repair, not the migration engine.
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: reportWith(4), showRowActions: false },
    });
    expect(queryByTestId('preflight-migrate-btn')).toBeNull();
  });

  // A player cannot act on `forgeconfigapiport`. The project name resolved per (dependent,
  // dep id) — by the Installed tab, or the gate as it opens — lives in the name store.
  it('renders the resolved dependency name from the name store', () => {
    storeNames.set('sha0:dep-0', 'Forge Config API Port');
    try {
      const { getByTestId } = render(PreflightPanel, {
        props: { report: reportWith(1), instanceId: 'i' },
      });
      const row = getByTestId('preflight-row');
      expect(row.textContent).toContain('Forge Config API Port');
      expect(row.textContent).not.toContain('dep-0');
    } finally {
      storeNames.clear();
    }
  });

  it('names the dependency in the install button too, not just the sentence', () => {
    storeNames.set('sha0:dep-0', 'Forge Config API Port');
    try {
      const { getByRole } = render(PreflightPanel, {
        props: { report: reportWith(1), instanceId: 'i' },
      });
      expect(getByRole('button', { name: /Forge Config API Port/ })).toBeTruthy();
    } finally {
      storeNames.clear();
    }
  });

  // 07c: on a cold start nothing has been resolved yet, and the report names the provider's own
  // row (`provider_name`) — a disabled jar or a range's provider never shows as its loader id.
  it('names a provider by the name the report gives its row, never by its loader id', () => {
    const disabled: DepViolation = {
      ...missing(0),
      kind: 'required_disabled',
      dependent_name: 'Zoomify',
      dep_id: 'yet_another_config_lib_v3',
      provider_sha1: 'yacl-sha',
      provider_name: 'YetAnotherConfigLib',
    };
    const range: DepViolation = { ...outOfRange(), provider_name: 'Sodium' };
    render(PreflightPanel, {
      props: { report: { violations: [disabled, range] }, instanceId: 'i', showRowActions: false },
    });
    const [first, second] = screen.getAllByTestId('preflight-row').map((r) => r.textContent ?? '');
    expect(first).toContain('YetAnotherConfigLib');
    expect(first).not.toContain('yet_another_config_lib_v3');
    expect(second).toContain('Sodium');
    expect(second).not.toMatch(/\bsodium\b/);
  });

  // Nothing resolved yet — a cold start: the id until a name arrives.
  it('falls back to the raw dep id while no name is known — the cold-start case', () => {
    const { getByTestId } = render(PreflightPanel, {
      props: { report: reportWith(1), showRowActions: false },
    });
    expect(getByTestId('preflight-row').textContent).toContain('dep-0');
  });

  it('falls back per row, so one unresolved id does not hide the others', () => {
    storeNames.set('sha0:dep-0', 'Forge Config API Port');
    try {
      const { getAllByTestId } = render(PreflightPanel, {
        props: { report: reportWith(2), instanceId: 'i' },
      });
      const rows = getAllByTestId('preflight-row');
      expect(rows[0]!.textContent).toContain('Forge Config API Port');
      expect(rows[1]!.textContent).toContain('dep-1');
    } finally {
      storeNames.clear();
    }
  });
});

describe('PreflightPanel — what stops the game', () => {
  const disabledDep = (): DepViolation => ({
    ...missing(0),
    kind: 'required_disabled',
    dependent_name: 'Waystones',
    dependent_sha1: 'w',
    dep_id: 'balm',
    provider_sha1: 'balm-sha',
    provider_name: 'Balm',
  });

  it('titles the panel with what it lists, danger border on the surface — no danger box', () => {
    const { getByTestId } = render(PreflightPanel, { props: { report: reportWith(1) } });
    const panel = getByTestId('preflight-panel');
    expect(panel.textContent).toContain('What stops the game');
    expect(panel.className).toContain('border-danger');
    expect(panel.className).toContain('bg-surface');
    expect(panel.className).not.toContain('bg-danger-bg');
    expect(panel.className).not.toContain('bg-warning-bg');
  });

  it('says a disabled dependency is disabled and offers Enable — never Install', async () => {
    const onEnableProvider = vi.fn();
    const v = disabledDep();
    render(PreflightPanel, {
      props: { report: { violations: [v] }, onEnableProvider },
    });
    expect(screen.getByTestId('preflight-row').textContent).toContain(
      "Waystones won't load: Balm is disabled",
    );
    expect(screen.queryByRole('button', { name: /install/i })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Enable' }));
    expect(onEnableProvider).toHaveBeenCalledWith(v);
  });

  // Plan §5b V1 (03 vs 01): a jar built for another platform needs another build of ITSELF —
  // the row offers «Choose version» (its own version list), and so does its panel row. The host
  // says how to open that list, or that there is none (a manual jar has no platform page).
  it('a platform mismatch offers the row’s «Choose version» — the mod’s own builds', async () => {
    const platform: DepViolation = {
      ...missing(3),
      kind: 'platform_mismatch',
      dependent_name: 'Better Third Person',
      dep_id: 'minecraft',
    };
    const open = vi.fn();
    const ownVersionOpener = vi.fn((v: DepViolation) =>
      v.dependent_sha1 === 'sha3' ? open : null,
    );
    const { unmount } = render(PreflightPanel, {
      props: { report: { violations: [platform] }, ownVersionOpener },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose version' }));
    expect(open).toHaveBeenCalledOnce();
    unmount();

    // No platform identity to pick a build from: no button, never a dead one.
    render(PreflightPanel, {
      props: {
        report: { violations: [{ ...platform, dependent_sha1: 'manual' }] },
        ownVersionOpener,
      },
    });
    expect(screen.queryByRole('button', { name: 'Choose version' })).toBeNull();
  });

  it('counts only fixable rows in «Fix all», and only when a handler is given', async () => {
    const onFixAll = vi.fn();
    const platform: DepViolation = {
      ...missing(9),
      kind: 'platform_mismatch',
      dep_id: 'minecraft',
    };
    const report = { violations: [missing(0), disabledDep(), platform] };
    const { unmount } = render(PreflightPanel, { props: { report } });
    expect(screen.queryByTestId('preflight-fix-all')).toBeNull();
    unmount();
    render(PreflightPanel, { props: { report, onFixAll } });
    const btn = screen.getByTestId('preflight-fix-all');
    expect(btn.textContent).toContain('Fix all (2)');
    await fireEvent.click(btn);
    expect(onFixAll).toHaveBeenCalledOnce();
  });

  it('jumps to the dependent row from ↗', async () => {
    const onJumpToDependent = vi.fn();
    render(PreflightPanel, { props: { report: reportWith(1), onJumpToDependent } });
    await fireEvent.click(screen.getByRole('button', { name: 'Show Mod 0 in the list' }));
    expect(onJumpToDependent).toHaveBeenCalledWith(
      expect.objectContaining({ dependent_sha1: 'sha0' }),
    );
  });

  it('stays quiet while a self-completing pack is still fetching files (the gate’s predicate)', () => {
    const { queryByTestId } = render(PreflightPanel, {
      props: {
        report: {
          violations: [missing(0)],
          pack_completion: { total: 2, outstanding: [{} as never] },
        },
      },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
  });

  it('an incompatibility-only panel does not claim anything stops the game', () => {
    const { getByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onMigrate: () => {}, migrateCount: 2 },
    });
    const panel = getByTestId('preflight-panel');
    expect(panel.textContent).toContain('Some mods may not work');
    expect(panel.textContent).not.toContain('What stops the game');
    expect(panel.className).toContain('border-warning-text');
  });

  it('names a dependency per dependent from the store — two mods sharing a mod-id (A-F3)', () => {
    storeNames.set('a:balm', 'Balm');
    storeNames.set('b:balm', 'Balm Port');
    try {
      render(PreflightPanel, {
        props: {
          instanceId: 'i1',
          report: {
            violations: [
              { ...missing(0), dependent_sha1: 'a', dep_id: 'balm' },
              { ...missing(1), dependent_sha1: 'b', dep_id: 'balm' },
            ],
          },
        },
      });
      const rows = screen.getAllByTestId('preflight-row').map((r) => r.textContent ?? '');
      expect(rows).toEqual([
        expect.stringContaining('needs Balm, which'),
        expect.stringContaining('needs Balm Port, which'),
      ]);
    } finally {
      storeNames.clear();
    }
  });
});

// The two-sided planner (spec §5.4, §6.5, D8): «Fix…» asks on click — network only then — and
// each offer names the change it makes.
describe('PreflightPanel — the planner', () => {
  const ver = (n: string) => ({ version_number: n }) as never;
  const keyOf = (v: DepViolation) => `${v.dependent_sha1}:${v.dep_id}`;
  const plansOf = (v: DepViolation, s: PlanState) => new Map([[keyOf(v), s]]);

  it('spins while the plan loads, and says what it is doing', () => {
    const v = outOfRange();
    render(PreflightPanel, {
      props: { report: { violations: [v] }, plans: plansOf(v, { status: 'loading' }) },
    });
    expect(screen.getByRole('status', { name: 'Looking for a fix…' })).toBeTruthy();
    expect(screen.queryByText('Fix…')).toBeNull();
  });

  it('offers both sides once ready — the dependent first — and applies the one clicked', async () => {
    const onApplyPlan = vi.fn();
    const v: DepViolation = { ...outOfRange(), provider_name: 'Sodium' };
    const plan = {
      update_dependent: { version: ver('3.1'), breaks: [] },
      change_provider: { version: ver('0.5.11'), direction: 'downgrade' as const, breaks: [] },
    };
    render(PreflightPanel, {
      props: {
        report: { violations: [v] },
        plans: plansOf(v, { status: 'ready', plan }),
        onApplyPlan,
      },
    });
    const dependent = screen.getByTestId('preflight-plan-dependent');
    expect(dependent.textContent?.trim()).toBe('Update indium to 3.1');
    expect(dependent.className).toContain('btn-primary');
    const provider = screen.getByTestId('preflight-plan-provider');
    expect(provider.textContent?.trim()).toBe('Roll Sodium back to 0.5.11');
    expect(screen.queryByText('Fix…')).toBeNull();
    // The manual path stays beside the offers.
    expect(screen.getByText('Choose version')).toBeTruthy();
    await fireEvent.click(provider);
    expect(onApplyPlan).toHaveBeenCalledWith(v, 'provider');
  });

  it('says whom a provider change would break; it stays secondary and takes its own click (D8)', () => {
    const v = outOfRange();
    const plan = {
      update_dependent: null,
      change_provider: {
        version: ver('0.5.11'),
        direction: 'downgrade' as const,
        breaks: ['Iris', 'Reese'],
      },
    };
    render(PreflightPanel, {
      props: { report: { violations: [v] }, plans: plansOf(v, { status: 'ready', plan }) },
    });
    const button = screen.getByTestId('preflight-plan-provider');
    const breaks = screen.getByTestId('preflight-plan-breaks');
    expect(breaks.textContent?.trim()).toBe('Breaks: Iris, Reese');
    expect(button.className).not.toContain('btn-primary');
    // Heard with the button, not only seen beside it.
    expect(describedText(button)).toBe('Breaks: Iris, Reese');
  });

  it('says whom a dependent update would break too — each offer its own; neither is the default (D8)', () => {
    const v = outOfRange();
    const plan = {
      update_dependent: { version: ver('3.1'), breaks: ['Pin'] },
      change_provider: {
        version: ver('0.5.11'),
        direction: 'downgrade' as const,
        breaks: ['Iris', 'Reese'],
      },
    };
    render(PreflightPanel, {
      props: { report: { violations: [v] }, plans: plansOf(v, { status: 'ready', plan }) },
    });
    const dependent = screen.getByTestId('preflight-plan-dependent');
    const provider = screen.getByTestId('preflight-plan-provider');
    expect(describedText(dependent)).toBe('Breaks: Pin');
    expect(describedText(provider)).toBe('Breaks: Iris, Reese');
    expect(dependent.className).not.toContain('btn-primary');
    expect(provider.className).not.toContain('btn-primary');
  });

  it('gives an incompatibility the same flow (it had no action before)', async () => {
    const onPlan = vi.fn();
    const v: DepViolation = { ...outOfRange(), kind: 'incompatible_installed' };
    render(PreflightPanel, { props: { report: { violations: [v] }, onPlan } });
    await fireEvent.click(screen.getByText('Fix…'));
    expect(onPlan).toHaveBeenCalledWith(v);
    expect(screen.queryByText('Choose version')).toBeNull();
  });

  it('gives a conflict whose provider the report could not link a way out too', async () => {
    // Only the picker needs the project; the planner reads the profile itself.
    const onPlan = vi.fn();
    const v: DepViolation = { ...outOfRange(), provider_project: null };
    render(PreflightPanel, { props: { report: { violations: [v] }, onPlan } });
    await fireEvent.click(screen.getByText('Fix…'));
    expect(onPlan).toHaveBeenCalledWith(v);
    expect(screen.queryByText('Choose version')).toBeNull();
  });

  it('says a look that failed failed, and why — never «no version» — and lets the user retry', async () => {
    const onPlan = vi.fn();
    const v = outOfRange();
    render(PreflightPanel, {
      props: {
        report: { violations: [v] },
        onPlan,
        plans: plansOf(v, { status: 'failed', message: 'No internet connection' }),
      },
    });
    expect(screen.getByText("Couldn't look for a fix: No internet connection")).toBeTruthy();
    expect(screen.queryByText('No compatible version')).toBeNull();
    // Heard where focus lands after the look: on the button that tries again.
    const retry = screen.getByRole('button', { name: 'Fix…' });
    expect(describedText(retry)).toBe("Couldn't look for a fix: No internet connection");
    await fireEvent.click(retry);
    expect(onPlan).toHaveBeenCalledWith(v);
  });

  it('a dead end with no linked project still offers to look for an alternative', () => {
    const v: DepViolation = { ...outOfRange(), provider_project: null };
    render(PreflightPanel, {
      props: { report: { violations: [v] }, deadEndKeys: new Set([keyOf(v)]) },
    });
    expect(screen.getByText('No compatible version')).toBeTruthy();
    expect(describedText(screen.getByRole('button', { name: 'Find alternative' }))).toBe(
      'No compatible version',
    );
    // There is no page to open without a project.
    expect(screen.queryByText('Open mod page')).toBeNull();
  });
});

// Plan §5c V3 (screenshot n03b): the reason was `flex-1` — a basis of 0 — beside fixed-width offers,
// so from 820 to 1280 px the offers took the line and left the reason a word per line. The reason
// keeps a readable width; its fixes sit beside it while both fit and wrap under it, as one group,
// when they do not. Only a browser lays this out: these pin the structure the re-render measures.
describe('PreflightPanel — a narrow row', () => {
  const ver = (n: string) => ({ version_number: n }) as never;

  function offersRow(): HTMLElement {
    const v: DepViolation = { ...outOfRange(), provider_name: 'Sodium' };
    const plan = {
      update_dependent: { version: ver('3.1'), breaks: [] },
      change_provider: {
        version: ver('0.5.11'),
        direction: 'downgrade' as const,
        breaks: ['Iris'],
      },
    };
    render(PreflightPanel, {
      props: {
        report: { violations: [v] },
        plans: new Map<string, PlanState>([
          [`${v.dependent_sha1}:${v.dep_id}`, { status: 'ready', plan }],
        ]),
        onJumpToDependent: () => {},
      },
    });
    return screen.getByTestId('preflight-row');
  }

  it('keeps the reason a readable width and wraps the fixes under it as one group', () => {
    const row = offersRow();
    const text = within(row).getByTestId('preflight-row-text');
    const actions = within(row).getByTestId('preflight-row-actions');
    // A basis of its own, never flex-1's 0 — and still free to shrink in a very narrow window.
    expect(text.className).toMatch(/\bbasis-72\b/);
    expect(text.className).toMatch(/\bgrow\b/);
    expect(text.className).toMatch(/\bmin-w-0\b/);
    expect(text.className).not.toMatch(/\bflex-1\b/);
    // The reason and its fixes share one line that wraps…
    const body = text.parentElement;
    expect(actions.parentElement).toBe(body);
    expect(body?.className).toMatch(/\bflex-wrap\b/);
    // …and every fix rides in the one group, which wraps in itself.
    expect(actions.className).toMatch(/\bflex-wrap\b/);
    for (const id of [
      'preflight-plan-dependent',
      'preflight-plan-provider',
      'preflight-plan-breaks',
    ])
      expect(actions.contains(screen.getByTestId(id))).toBe(true);
    expect(actions.contains(screen.getByText('Choose version'))).toBe(true);
  });

  it("keeps the icon on the reason's first line and ↗ at the row's end", () => {
    const row = offersRow();
    // By baseline: the icon's line sits on the reason's first line, whether the fixes share that
    // line or wrap under it.
    expect(row.className).toMatch(/\bitems-baseline\b/);
    const [icon, body, jump] = [...row.children];
    expect(icon?.querySelector('svg')).not.toBeNull();
    expect(body?.contains(within(row).getByTestId('preflight-row-text'))).toBe(true);
    expect(jump).toBe(within(row).getByRole('button', { name: 'Show indium in the list' }));
    expect(jump?.className).toMatch(/\bself-center\b/);
  });
});
