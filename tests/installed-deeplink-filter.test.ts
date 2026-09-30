/**
 * The Overview's attention item deep-links into the Installed tab's «Проблемы» view — since the
 * 2026-09-28 program the ONE problem view (`issues`), into which the old «Несовместимые» view was
 * folded. `requestedFilter` shipped in #332 with no test, and the filter was in fact reverted
 * before a single row existed:
 *
 *   MainTabs renders AddonsTab under {#if active === 'mod_browser'}, so arriving from the Overview
 *   always MOUNTS it fresh → `data.rows` is `[]` for the whole mount flush → the view's count is 0
 *   (the predicate has nothing to run over) → writing `viewFilter` re-runs the auto-reset effect →
 *   it reads that 0 as "none" and reverts to 'all'.
 *
 * The view is now fed by the pre-flight too, which can answer after the rows. These cases pin
 * both halves: the filter survives a not-yet-loaded list — rows or pre-flight — and still falls
 * back when the set really is empty (which is what the auto-reset exists for).
 */
import { render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mod = vi.hoisted(() => (sha1: string, projectId: string, name: string) => ({
  filename: `${sha1}.jar`,
  sha1,
  source: 'modrinth',
  project_id: projectId,
  version_id: 'v',
  name,
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
  requires: [],
}));

const mocks = vi.hoisted(() => ({
  scanInstanceModCompat: vi.fn(),
  instanceDependencyPreflight: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [mod('a', 'PA', 'Alpha'), mod('b', 'PB', 'Bravo')],
    }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: mocks.instanceDependencyPreflight,
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: mocks.scanInstanceModCompat,
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

import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// A manual suspect: loader-mismatched and not live-checkable — a compat warning.
const mismatch = (sha1: string) => ({
  sha1,
  loader_mismatch: true,
  live_checkable: false,
  detected_loader: 'Fabric',
});
const missing = (sha1: string) => ({
  dependent_sha1: sha1,
  dependent_name: 'Alpha',
  dep_id: 'lib',
  kind: 'missing_required',
  installed_version: null,
  needed: '',
  needed_desc: { raw: '', family: 'maven', alternatives: [], unparseable: false, soft: false },
  provider_project: null,
  provider_sha1: null,
  family: null,
});
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
  requestedFilter: 'issues' as const,
});
const row = (pid: string) => document.querySelector(`[data-mod-row="modrinth:${pid}"]`);

describe('Overview deep-link into the issues view', () => {
  beforeEach(() => {
    mocks.scanInstanceModCompat.mockReset();
    mocks.instanceDependencyPreflight.mockReset();
    mocks.instanceDependencyPreflight.mockResolvedValue({ status: 'ok', data: { violations: [] } });
    // The scan is an app-wide singleton shared with the Overview.
    invalidateCompatScan();
  });

  it('keeps the requested filter when the row list has not loaded yet', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [mismatch('a')] });
    render(InstalledModsView, { props: props('i') });
    // Alpha is the problem; Bravo must stay filtered out. Before #332's fix both rendered.
    await waitFor(() => expect(row('PA')).not.toBeNull());
    expect(row('PB')).toBeNull();
  });

  it('keeps it while the pre-flight has not answered yet', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
    let answer: (r: unknown) => void = () => {};
    mocks.instanceDependencyPreflight.mockReturnValue(
      new Promise((r) => {
        answer = r;
      }),
    );
    render(InstalledModsView, { props: props('late-preflight') });
    // Rows are in (the toolbar's chip group renders only then); give the auto-reset its turn.
    await waitFor(() =>
      expect(document.querySelector('[data-testid="installed-filter-all"]')).not.toBeNull(),
    );
    await new Promise((r) => setTimeout(r, 0));
    answer({ status: 'ok', data: { violations: [missing('a')] } });
    await waitFor(() => expect(row('PA')).not.toBeNull());
    expect(row('PB')).toBeNull();
  });

  it('still falls back to "all" when nothing is actually wrong', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
    render(InstalledModsView, { props: props('i') });
    // A stale link must not strand the user on an empty view once the list IS loaded.
    await waitFor(() => {
      expect(row('PA')).not.toBeNull();
      expect(row('PB')).not.toBeNull();
    });
  });
});
