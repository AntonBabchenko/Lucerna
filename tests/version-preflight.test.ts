import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { DepTreeNode, DepViolation, PreflightReport } from '$lib/ipc/bindings';
import DepTree from '$lib/mods/DepTree.svelte';
import { EMPTY_TREE_CTX, edgeConflict } from '$lib/mods/dep-node-state';
import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
import { hasBlocking } from '$lib/mods/preflight.svelte';
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

// A tree node is out of range per EDGE (plan §5b V1): under the dependent whose own range rejects
// the installed provider, keyed by the provider's platform project — never under every mod that
// shows the project.
const core = { source: 'modrinth' as const, project_id: 'core-id' };

describe('preflight overlay mapping', () => {
  it("marks the provider's project under the dependent that declared the range", () => {
    expect(edgeConflict(report.violations, core, 'aa')).toBe(report.violations[0]);
  });
  it('treats a non-empty report as blocking', () => {
    expect(hasBlocking(report)).toBe(true);
    expect(hasBlocking({ violations: [] })).toBe(false);
  });
});

describe('edgeConflict edge cases', () => {
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
    expect(edgeConflict(missingReport.violations, core, 'bb')).toBeNull();
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
    expect(edgeConflict(noProviderReport.violations, core, 'cc')).toBeNull();
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
    // A CurseForge node keys its numeric mod id as the project id.
    const cf = { source: 'curseforge' as const, project_id: '12345' };
    expect(edgeConflict(cfReport.violations, cf, 'dd')).toBe(cfReport.violations[0]);
  });

  it('marks nothing for an empty report, or where no mod declared the edge', () => {
    expect(edgeConflict([], core, 'aa')).toBeNull();
    // Under an absent parent nothing installed declared the node.
    expect(edgeConflict(report.violations, core, null)).toBeNull();
  });
});

// A tree node's «Fix…» plans the conflict behind its mark (spec §6.5) — the node's own dependent's,
// the only one that marks it.
describe('edgeConflict', () => {
  const own = report.violations[0] as DepViolation;
  const other: DepViolation = { ...own, dependent_sha1: 'bb', dependent_name: 'Other' };

  it('is the conflict this dependent declared on the node’s project', () => {
    expect(edgeConflict([other, own], core, 'aa')).toBe(own);
  });

  it('never lends another dependent’s conflict on the same project to this edge (06d)', () => {
    expect(edgeConflict([other], core, 'aa')).toBeNull();
    expect(edgeConflict([other], core, null)).toBeNull();
    expect(edgeConflict([other], core, 'bb')).toBe(other);
  });

  it('is null for a project this dependent’s ranges do not name', () => {
    const cf = { source: 'curseforge' as const, project_id: '12345' };
    const cfConflict: DepViolation = {
      ...own,
      provider_project: { source: 'curseforge', mod_id: 12345, file_id: null },
    };
    expect(edgeConflict([own], { ...core, project_id: 'else' }, 'aa')).toBeNull();
    // A CurseForge node keys its numeric mod id as the project id.
    expect(edgeConflict([cfConflict], cf, 'aa')).toBe(cfConflict);
    // An incompatibility or a conflict with no linked project marks no node.
    expect(edgeConflict([{ ...own, kind: 'incompatible_installed' }], core, 'aa')).toBeNull();
    expect(edgeConflict([{ ...own, provider_project: null }], core, 'aa')).toBeNull();
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

  // A range's provider is a row the backend names (`provider_name`, the registry's name for the
  // jar `provider_sha1` names — plan §5b V1); the row's job — naming both sides in words — is
  // what this asserts.
  it('renders one row for a version_out_of_range violation with the dependent name and dep name', () => {
    const { getByTestId, getAllByTestId } = render(PreflightPanel, {
      props: {
        report: {
          violations: [
            {
              ...outOfRangeViolation,
              provider_sha1: 'core-sha',
              provider_name: 'Sophisticated Core',
            },
          ],
        },
      },
    });
    expect(getByTestId('preflight-panel')).toBeTruthy();
    const rows = getAllByTestId('preflight-row');
    expect(rows).toHaveLength(1);
    const rowText = rows[0].textContent ?? '';
    expect(rowText).toContain('Sophisticated Backpacks');
    expect(rowText).toContain('Sophisticated Core');
  });

  it('shows the raw dep id when nothing names the dependency', () => {
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
// DepTree overlay — a version mismatch replaces the green "installed" check, per edge
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

// The tree reads the report per edge: `dependentSha1` is the mod that declared this level.
const treeProps = (dependentSha1: string, violations: DepViolation[]) => ({
  hoveredKey: null,
  onHover: () => {},
  onInstall: () => {},
  onAdd: () => {},
  onOpenDetail: () => {},
  dependentSha1,
  ctx: { ...EMPTY_TREE_CTX, report: { violations } },
});

describe('DepTree overlay', () => {
  it('shows treeOutOfRange text and hides installedStatus under the dependent whose range rejects it', () => {
    const { getByText, queryByText } = render(DepTree, {
      props: { nodes: [satisfiedNode], ...treeProps('aa', report.violations) },
    });
    // Direction-neutral: the overlay also fires for an UPPER bound, where
    // "too old" would be the opposite of the truth.
    expect(getByText('version mismatch')).toBeTruthy();
    expect(queryByText('installed')).toBeNull();
  });

  it('shows installedStatus (green check) when nothing is out of range', () => {
    const { getByText, queryByText } = render(DepTree, {
      props: { nodes: [satisfiedNode], ...treeProps('aa', []) },
    });
    expect(getByText('installed')).toBeTruthy();
    expect(queryByText('version mismatch')).toBeNull();
  });

  it("shows installedStatus under a mod whose own ranges accept it, whoever else's does not", () => {
    const { getByText, queryByText } = render(DepTree, {
      props: { nodes: [satisfiedNode], ...treeProps('zz', report.violations) },
    });
    expect(getByText('installed')).toBeTruthy();
    expect(queryByText('version mismatch')).toBeNull();
  });
});
