/**
 * Tests for PreflightPanel: a long violation list must be wrapped in a
 * scrollable container so it cannot push the panel (or, when reused inside
 * the launch gate, the dialog's footer buttons) past the window edge.
 *
 * i18n resolves to real EN strings in the test environment.
 */
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
import { rangeDesc, rawRangeDesc } from './test-utils/range-desc';

function missing(i: number): DepViolation {
  return {
    kind: 'missing_required',
    dependent_name: `Mod ${i}`,
    dependent_sha1: `sha${i}`,
    dep_id: `dep-${i}`,
    dep_display_name: null,
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
    dep_display_name: null,
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
      dep_display_name: null,
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
      props: { report: { violations: [incompatible()] }, onUpdate: () => {} },
    });
    const row = getByTestId('preflight-row').textContent ?? '';
    expect(row).toContain('is incompatible with create 6.0.9 or older');
    expect(row).toContain('installed: 6.0.5');
    expect(row).not.toContain('(,6.0.9]');
  });

  it('offers no version remediation for an incompatibility', () => {
    // "Update" routes through modsFilterSatisfying, whose satisfying set is
    // exactly the versions that clash here — it would install a worse one.
    const { queryByText } = render(PreflightPanel, {
      props: { report: { violations: [incompatible()] }, onUpdate: () => {} },
    });
    expect(queryByText('Update')).toBeNull();
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
      props: { report: { violations: [v] }, onUpdate: () => {} },
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
      props: { report: { violations: [bare] }, onUpdate: () => {} },
    });
    expect(getByTestId('preflight-row').textContent).not.toContain('any version works');
  });

  it('words an out-of-range optional dependency as support, not a requirement', () => {
    const v: DepViolation = {
      ...incompatible(),
      kind: 'optional_out_of_range',
      dep_id: 'curios',
      needed: '[9.0,)',
      needed_desc: rangeDesc('[9.0,)', [{ kind: 'at_least', version: '9.0' }]),
      installed_version: '5.4.0',
    };
    const { getByTestId } = render(PreflightPanel, {
      props: { report: { violations: [v] }, onUpdate: () => {} },
    });
    expect(getByTestId('preflight-row').textContent).toContain('supports curios 9.0 or newer');
  });
});

describe('PreflightPanel', () => {
  it('renders nothing when there are no violations', () => {
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onUpdate: () => {} },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
  });

  it('wraps the rows in a scrollable container', () => {
    const { getByTestId } = render(PreflightPanel, {
      props: { report: reportWith(40), onUpdate: () => {} },
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
      props: { report: reportWith(40), onUpdate: () => {} },
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
          dep_display_name: null,
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
    render(PreflightPanel, { props: { report, onUpdate: () => {}, onInstallMissing } });
    const btn = screen.getByRole('button', { name: /balm/i });
    await fireEvent.click(btn);
    expect(onInstallMissing).toHaveBeenCalledWith(report.violations[0]);
  });

  it('shows Update + Choose-version on an actionable out-of-range row', () => {
    const report: PreflightReport = { violations: [outOfRange()] };
    const { getByText } = render(PreflightPanel, { props: { report, onUpdate: () => {} } });
    expect(getByText('Update')).toBeTruthy();
    expect(getByText('Choose version')).toBeTruthy();
  });

  it('calls onChooseVersion when Choose-version is clicked', async () => {
    const onChooseVersion = vi.fn();
    const v = outOfRange();
    const report: PreflightReport = { violations: [v] };
    render(PreflightPanel, { props: { report, onUpdate: () => {}, onChooseVersion } });
    await fireEvent.click(screen.getByText('Choose version'));
    expect(onChooseVersion).toHaveBeenCalledWith(v);
  });

  it('shows the dead-end actions (no Update) when the row is in deadEndKeys', () => {
    const v = outOfRange();
    const report: PreflightReport = { violations: [v] };
    const { getByText, queryByText } = render(PreflightPanel, {
      props: {
        report,
        onUpdate: () => {},
        deadEndKeys: new Set([`${v.dependent_sha1}:${v.dep_id}`]),
      },
    });
    expect(queryByText('Update')).toBeNull();
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
        onUpdate: () => {},
        busyKeys: new Set([`${v.dependent_sha1}:${v.dep_id}`]),
      },
    });
    expect(queryByText('Update')).toBeNull();
    expect(queryByText('Choose version')).toBeNull();
    expect(getByRole('status')).toBeTruthy();
  });

  it('hides all per-row actions when showRowActions is false (launch-gate mode)', () => {
    const report: PreflightReport = { violations: [outOfRange(), missing(0)] };
    const { queryByText, queryAllByRole } = render(PreflightPanel, {
      props: { report, onUpdate: () => {}, showRowActions: false },
    });
    expect(queryByText('Update')).toBeNull();
    expect(queryByText('Choose version')).toBeNull();
    expect(queryAllByRole('button')).toHaveLength(0);
  });
});

describe('PreflightPanel bulk migrate entry', () => {
  it('renders a header migrate button and calls onMigrate when count > 0', async () => {
    const onMigrate = vi.fn();
    render(PreflightPanel, {
      props: { report: reportWith(3), onUpdate: () => {}, onMigrate, migrateCount: 3 },
    });
    const btn = screen.getByTestId('preflight-migrate-btn');
    expect(btn.textContent).toContain('Fix incompatible mods');
    await fireEvent.click(btn);
    expect(onMigrate).toHaveBeenCalledTimes(1);
  });

  it('shows the panel (header + button, no rows) when there are incompatibilities but no dep violations', () => {
    // compat.incompatibleCount is authoritative and can be positive while the
    // dependency-graph report is empty (a jar's MC range read off metadata, an
    // MC dep declared as a recommendation). The bulk entry must still appear.
    const onMigrate = vi.fn();
    const { getByTestId, queryAllByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onUpdate: () => {}, onMigrate, migrateCount: 2 },
    });
    expect(getByTestId('preflight-panel')).toBeTruthy();
    expect(getByTestId('preflight-migrate-btn')).toBeTruthy();
    expect(queryAllByTestId('preflight-row')).toHaveLength(0);
  });

  it('renders nothing when there are no violations and no incompatibilities', () => {
    const onMigrate = vi.fn();
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onUpdate: () => {}, onMigrate, migrateCount: 0 },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
    expect(queryByTestId('preflight-migrate-btn')).toBeNull();
  });

  it('shows no migrate button in launch-gate shape (no onMigrate) even with violations', () => {
    // The launch gate reuses PreflightPanel without the migration props; its
    // remediation is its own "Update & launch" batch, not the migration engine.
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: reportWith(4), onUpdate: () => {}, showRowActions: false },
    });
    expect(queryByTestId('preflight-migrate-btn')).toBeNull();
  });
});
