import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { DepTreeNode, DepViolation, PreflightReport } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
import { hasBlocking, overlayConflict, toOverlayKeys } from '$lib/mods/preflight.svelte';
import { rawRangeDesc } from './test-utils/range-desc';

const report: PreflightReport = {
  violations: [
    {
      kind: 'version_out_of_range',
      dependent_name: 'Sophisticated Backpacks',
      dependent_sha1: 'aa',
      dep_id: 'sophisticatedcore',
      needed: '[1.3.51,)',
      needed_desc: rawRangeDesc('[1.3.51,)'),
      installed_version: '1.3.50.2005',
      provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
      provider_sha1: null,
      family: 'maven',
    },
  ],
};

describe('preflight overlay mapping', () => {
  it('exposes modrinth:core-id as an out-of-range overlay key', () => {
    expect(toOverlayKeys(report)).toContain('modrinth:core-id');
  });
  it('treats a non-empty report as blocking', () => {
    expect(hasBlocking(report)).toBe(true);
    expect(hasBlocking({ violations: [] })).toBe(false);
  });
});

describe('toOverlayKeys edge cases', () => {
  it('ignores missing_required violations (no provider_project needed)', () => {
    const missingReport: PreflightReport = {
      violations: [
        {
          kind: 'missing_required',
          dependent_name: 'Backpacks',
          dependent_sha1: 'bb',
          dep_id: 'missingmod',
          needed: '',
          needed_desc: rawRangeDesc(''),
          installed_version: null,
          provider_project: null,
          provider_sha1: null,
          family: null,
        },
      ],
    };
    expect(toOverlayKeys(missingReport).size).toBe(0);
  });

  it('ignores version_out_of_range violations with no provider_project', () => {
    const noProviderReport: PreflightReport = {
      violations: [
        {
          kind: 'version_out_of_range',
          dependent_name: 'SomeMod',
          dependent_sha1: 'cc',
          dep_id: 'unknowndep',
          needed: '[1.0,)',
          needed_desc: rawRangeDesc('[1.0,)'),
          installed_version: '0.9',
          provider_project: null,
          provider_sha1: null,
          family: 'maven',
        },
      ],
    };
    expect(toOverlayKeys(noProviderReport).size).toBe(0);
  });

  it('maps curseforge provider_project to curseforge:mod_id', () => {
    const cfReport: PreflightReport = {
      violations: [
        {
          kind: 'version_out_of_range',
          dependent_name: 'SomeMod',
          dependent_sha1: 'dd',
          dep_id: 'cfmod',
          needed: '[2.0,)',
          needed_desc: rawRangeDesc('[2.0,)'),
          installed_version: '1.9',
          provider_project: { source: 'curseforge', mod_id: 12345, file_id: null },
          provider_sha1: null,
          family: 'maven',
        },
      ],
    };
    expect(toOverlayKeys(cfReport)).toContain('curseforge:12345');
  });

  it('returns an empty set for an empty report', () => {
    expect(toOverlayKeys({ violations: [] }).size).toBe(0);
  });
});

// The overlay marks a PROJECT; a tree node's «Fix…» needs the conflict behind the mark — the one
// the node's own dependent declared when there is one (spec §6.5).
describe('overlayConflict', () => {
  const core = { source: 'modrinth' as const, project_id: 'core-id' };
  const own = report.violations[0] as DepViolation;
  const other: DepViolation = { ...own, dependent_sha1: 'bb', dependent_name: 'Other' };

  it('is the conflict this dependent declared on the node’s project', () => {
    expect(overlayConflict([other, own], core, 'aa')).toBe(own);
  });

  it('falls back to another dependent’s conflict on the project — the one the mark stands for', () => {
    expect(overlayConflict([other], core, 'aa')).toBe(other);
    expect(overlayConflict([other], core, null)).toBe(other);
  });

  it('is null for a project nothing marks — exactly the overlay’s rule', () => {
    const cf = { source: 'curseforge' as const, project_id: '12345' };
    const cfConflict: DepViolation = {
      ...own,
      provider_project: { source: 'curseforge', mod_id: 12345, file_id: null },
    };
    expect(overlayConflict([own], { ...core, project_id: 'else' }, 'aa')).toBeNull();
    // A CurseForge node keys its numeric mod id as the project id.
    expect(overlayConflict([cfConflict], cf, 'aa')).toBe(cfConflict);
    // An incompatibility or a conflict with no linked project marks no node.
    expect(overlayConflict([{ ...own, kind: 'incompatible_installed' }], core, 'aa')).toBeNull();
    expect(overlayConflict([{ ...own, provider_project: null }], core, 'aa')).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// PreflightPanel component rendering
// ---------------------------------------------------------------------------

const outOfRangeViolation: DepViolation = {
  kind: 'version_out_of_range',
  dependent_name: 'Sophisticated Backpacks',
  dependent_sha1: 'aa',
  dep_id: 'sophisticatedcore',
  needed: '[1.3.51,)',
  needed_desc: rawRangeDesc('[1.3.51,)'),
  installed_version: '1.3.50.2005',
  provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
  provider_sha1: null,
  family: 'maven',
};

const missingViolation: DepViolation = {
  kind: 'missing_required',
  dependent_name: 'Backpacks',
  dependent_sha1: 'bb',
  dep_id: 'missingmod',
  needed: '',
  needed_desc: rawRangeDesc(''),
  installed_version: null,
  provider_project: null,
  provider_sha1: null,
  family: null,
};

describe('PreflightPanel', () => {
  it('renders nothing when report is null', () => {
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: null },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
  });

  it('renders nothing when violations list is empty', () => {
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] } },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
  });

  // The dep's human name used to ride on the violation itself
  // (`dep_display_name`), a field the backend never populated. It now arrives
  // as a lookup the Installed tab resolves and passes down; the row's job —
  // naming both sides in words — is unchanged, which is what this asserts.
  it('renders one row for a version_out_of_range violation with the dependent name and dep name', () => {
    const { getByTestId, getAllByTestId } = render(PreflightPanel, {
      props: {
        report: { violations: [outOfRangeViolation] },
        depName: (v: { dep_id: string }) =>
          v.dep_id === 'sophisticatedcore' ? 'Sophisticated Core' : null,
      },
    });
    expect(getByTestId('preflight-panel')).toBeTruthy();
    const rows = getAllByTestId('preflight-row');
    expect(rows).toHaveLength(1);
    const rowText = rows[0].textContent ?? '';
    expect(rowText).toContain('Sophisticated Backpacks');
    expect(rowText).toContain('Sophisticated Core');
  });

  it('shows the raw dep id when no overlay is supplied — the launch-gate case', () => {
    const { getAllByTestId } = render(PreflightPanel, {
      props: { report: { violations: [outOfRangeViolation] } },
    });
    expect(getAllByTestId('preflight-row')[0].textContent).toContain('sophisticatedcore');
  });

  it('renders one row for a missing_required violation with the dependent name and dep id', () => {
    const { getAllByTestId } = render(PreflightPanel, {
      props: { report: { violations: [missingViolation] } },
    });
    const rows = getAllByTestId('preflight-row');
    expect(rows).toHaveLength(1);
    const rowText = rows[0].textContent ?? '';
    expect(rowText).toContain('Backpacks');
    expect(rowText).toContain('missingmod');
  });

  it('renders Fix… for version_out_of_range and an Install button for missing_required', () => {
    const reportWithBoth: PreflightReport = { violations: [outOfRangeViolation, missingViolation] };
    const { getAllByRole, getByRole, queryByRole } = render(PreflightPanel, {
      props: { report: reportWithBoth, onInstallMissing: () => {} },
    });
    const buttons = getAllByRole('button');
    // outOfRangeViolation → «Fix…» (the planner) + "Choose version"; missingViolation →
    // one "Install {dep}" button. Three action buttons total.
    expect(buttons).toHaveLength(3);
    expect(getByRole('button', { name: 'Fix…' })).toBeTruthy();
    expect(queryByRole('button', { name: /update/i })).toBeNull();
    expect(getByRole('button', { name: /choose version/i })).toBeTruthy();
    expect(getByRole('button', { name: /missingmod/i })).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// DepTree overlay — outOfRangeKeys replaces the green "installed" check
// ---------------------------------------------------------------------------

const satisfiedNode: DepTreeNode = {
  source: 'modrinth',
  project_id: 'core-id',
  name: 'Sophisticated Core',
  installed: true,
  declared: 'required',
  cycle: false,
  children: [],
};

const treeProps = {
  hoveredKey: null,
  onHover: () => {},
  onInstall: () => {},
  onAdd: () => {},
  onOpenDetail: () => {},
};

describe('DepTree overlay', () => {
  it('shows treeOutOfRange text and hides installedStatus when key is in outOfRangeKeys', () => {
    const outOfRangeKeys = new Set(['modrinth:core-id']);
    const { getByText, queryByText } = render(DepTree, {
      props: { nodes: [satisfiedNode], outOfRangeKeys, ...treeProps },
    });
    // Direction-neutral: the overlay also fires for an UPPER bound, where
    // "too old" would be the opposite of the truth.
    expect(getByText('version mismatch')).toBeTruthy();
    expect(queryByText('installed')).toBeNull();
  });

  it('shows installedStatus (green check) when outOfRangeKeys is empty (default)', () => {
    const { getByText, queryByText } = render(DepTree, {
      props: { nodes: [satisfiedNode], ...treeProps },
    });
    expect(getByText('installed')).toBeTruthy();
    expect(queryByText('version too old')).toBeNull();
  });

  it('shows installedStatus when outOfRangeKeys does not contain the node key', () => {
    const outOfRangeKeys = new Set(['modrinth:some-other-id']);
    const { getByText, queryByText } = render(DepTree, {
      props: { nodes: [satisfiedNode], outOfRangeKeys, ...treeProps },
    });
    expect(getByText('installed')).toBeTruthy();
    expect(queryByText('version too old')).toBeNull();
  });
});
