/**
 * The «Проблемы» quick-filter is fed by the pre-flight, never by the graph.
 *
 * The graph reports what an author typed on the platform. The pre-flight reads
 * the descriptor the loader actually opens. A measured mod declares a required
 * dependency on Modrinth that its own `neoforge.mods.toml` does not declare —
 * the loader never asks for it and the pack runs — so a graph-absent required
 * child must contribute nothing to the issue count, and a pre-flight violation
 * must contribute one.
 *
 * Both directions are pinned: a graph-only claim (0) and a pre-flight-only
 * violation (1) on the SAME row, so neither source can be swapped for the other
 * without one of the two failing.
 */
import { render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const mod = vi.hoisted(() => ({
  filename: 'a.jar',
  sha1: 'a',
  source: 'modrinth',
  project_id: 'PA',
  version_id: 'v',
  name: 'Alpha',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
  requires: [],
}));

// The platform says Alpha requires Stylish Effects; nothing is installed for it.
const graphWithAbsentRequired = vi.hoisted(() => ({
  roots: [
    {
      sha1: 'a',
      source: 'modrinth',
      project_id: 'PA',
      name: 'Alpha',
      required: [
        {
          source: 'modrinth',
          project_id: 'PB',
          name: 'Stylish Effects',
          installed: false,
          declared: 'required',
          cycle: false,
          children: [],
        },
      ],
      optional: [],
    },
  ],
}));

const mocks = vi.hoisted(() => ({ instanceDependencyPreflight: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [mod] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: graphWithAbsentRequired }),
    instanceDependencyPreflight: mocks.instanceDependencyPreflight,
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// A DISTINCT instance id per case: `preflightCache` is a per-instance LRU, so
// reusing one id makes the second render seed from the first case's report and
// never call the command at all.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
});

const issuesChip = () =>
  [...document.querySelectorAll('button')].find((b) => /issues|проблем/i.test(b.textContent ?? ''));

describe('the issue count comes from the pre-flight', () => {
  it('a graph-absent required dependency does not create an issue', async () => {
    mocks.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: { violations: [] },
    });
    render(InstalledModsView, { props: props('graph-only') });
    await waitFor(() => {
      expect(document.querySelector('[data-mod-row="modrinth:PA"]')).not.toBeNull();
    });
    expect(issuesChip()).toBeUndefined();

    // Case A, measured: the claim is not hidden either — it is reported with
    // attribution, in the neutral register, so the user can see what the author
    // typed without the launcher adopting it as a finding.
    await waitFor(() =>
      expect(document.querySelector('[data-testid="author-claim-badge"]')).not.toBeNull(),
    );
    expect(document.querySelector('[data-testid="status-badge"]')).toBeNull();
  });

  it('a pre-flight violation on the same row does create one', async () => {
    mocks.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: {
        violations: [
          {
            dependent_sha1: 'a',
            dependent_name: 'Alpha',
            dep_id: 'stylisheffects',
            kind: 'missing_required',
            installed_version: null,
            needed: '',
            needed_desc: {
              raw: '',
              family: 'maven',
              alternatives: [],
              unparseable: false,
              soft: false,
            },
            provider_project: null,
            provider_sha1: null,
            family: null,
          },
        ],
      },
    });
    render(InstalledModsView, { props: props('preflight-hit') });
    await waitFor(() => expect(issuesChip()).not.toBeUndefined());
  });
});
