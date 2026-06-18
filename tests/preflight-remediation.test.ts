/**
 * Tests for the remediation helpers (remediateViolation, remediateAll) and
 * the launch decision helper (decideLaunch) added in Task 12.
 *
 * Commands are mocked via vi.hoisted + vi.mock so the module under test
 * receives the fake implementations from the very first import.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';

// ---------------------------------------------------------------------------
// Hoisted mocks — must be declared before any imports that use them
// ---------------------------------------------------------------------------

const mocks = vi.hoisted(() => ({
  modsVersions: vi.fn(),
  modsFilterSatisfying: vi.fn(),
  modsInstallWithDeps: vi.fn(),
  modsUpdateOne: vi.fn(),
  instanceDependencyPreflight: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
// preflight-cache is imported by preflight.svelte; provide a no-op
vi.mock('$lib/mods/preflight-cache', () => ({
  preflightCache: { get: vi.fn(), set: vi.fn(), delete: vi.fn() },
}));

import {
  decideLaunch,
  remediateAll,
  remediatePickedVersion,
  remediateViolation,
  violationKey,
} from '$lib/mods/preflight.svelte';

// ---------------------------------------------------------------------------
// Shared violation fixtures
// ---------------------------------------------------------------------------

const modrinthViolation: DepViolation = {
  kind: 'version_out_of_range',
  dependent_name: 'Sophisticated Backpacks',
  dependent_sha1: 'aa',
  dep_id: 'sophisticatedcore',
  dep_display_name: 'Sophisticated Core',
  needed: '[1.3.51,)',
  installed_version: '1.3.50',
  provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
  provider_sha1: null,
  family: 'maven',
};

const curseforgeViolation: DepViolation = {
  kind: 'version_out_of_range',
  dependent_name: 'SomeMod',
  dependent_sha1: 'bb',
  dep_id: 'somecfdep',
  dep_display_name: null,
  needed: '[2.0,)',
  installed_version: '1.9',
  provider_project: { source: 'curseforge', mod_id: 99999, file_id: null },
  provider_sha1: null,
  family: 'maven',
};

const noProviderViolation: DepViolation = {
  kind: 'version_out_of_range',
  dependent_name: 'UnknownMod',
  dependent_sha1: 'cc',
  dep_id: 'unknowndep',
  dep_display_name: null,
  needed: '[1.0,)',
  installed_version: '0.9',
  provider_project: null,
  provider_sha1: null,
  family: 'maven',
};

const missingViolation: DepViolation = {
  kind: 'missing_required',
  dependent_name: 'Backpacks',
  dependent_sha1: 'dd',
  dep_id: 'missingmod',
  dep_display_name: null,
  needed: '',
  installed_version: null,
  provider_project: null,
  provider_sha1: null,
  family: null,
};

const fakeVersion = {
  source: 'modrinth' as const,
  project_id: 'core-id',
  version_id: 'v-new',
  name: 'Sophisticated Core',
  version_number: '1.3.52',
  mc_versions: ['1.20.1'],
  loaders: ['fabric' as const],
  primary_file: {
    url: 'https://cdn.modrinth.com/fake.jar',
    filename: 'fake.jar',
    sha1: 'ffff',
    size: 1024,
    distribution_allowed: true,
  },
  deps: [],
  published_at: null,
};

// ---------------------------------------------------------------------------
// remediateViolation
// ---------------------------------------------------------------------------

describe('remediateViolation', () => {
  beforeEach(() => {
    mocks.modsVersions.mockReset();
    mocks.modsFilterSatisfying.mockReset();
    mocks.modsInstallWithDeps.mockReset();
    mocks.modsUpdateOne.mockReset();
    // Default: the newest (index 0) satisfies the range. Tests that need a
    // different satisfying set override this.
    mocks.modsFilterSatisfying.mockResolvedValue([0]);
  });

  it('returns { ok: false, reason: "no-provider" } when provider_project is null', async () => {
    const result = await remediateViolation('inst-1', noProviderViolation, '1.20.1', 'fabric');
    expect(result).toEqual({ ok: false, reason: 'no-provider' });
    expect(mocks.modsVersions).not.toHaveBeenCalled();
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('returns { ok: false, reason: "no-provider" } for a missing_required violation with no provider', async () => {
    const result = await remediateViolation('inst-1', missingViolation, '1.20.1', 'fabric');
    expect(result).toEqual({ ok: false, reason: 'no-provider' });
  });

  it('returns { ok: false, reason: "no-version" } when modsVersions returns an error', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'error', error: 'network failure' });
    const result = await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');
    expect(result).toEqual({ ok: false, reason: 'no-version' });
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('returns { ok: false, reason: "no-version" } when modsVersions returns an empty list', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [] });
    const result = await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');
    expect(result).toEqual({ ok: false, reason: 'no-version' });
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('calls modsVersions with correct source + projectId for a modrinth violation', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Core', installed_dependencies: [] },
    });

    await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');

    expect(mocks.modsVersions).toHaveBeenCalledWith('modrinth', 'core-id', '1.20.1', 'fabric');
  });

  it('calls modsVersions with correct source + projectId (stringified mod_id) for a curseforge violation', async () => {
    const cfVersion = { ...fakeVersion, source: 'curseforge' as const, project_id: '99999' };
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [cfVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'CFDep', installed_dependencies: [] },
    });

    await remediateViolation('inst-1', curseforgeViolation, '1.20.1', 'fabric');

    expect(mocks.modsVersions).toHaveBeenCalledWith('curseforge', '99999', '1.20.1', 'fabric');
  });

  it('calls modsInstallWithDeps with the first version returned (newest-compatible-first)', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Core', installed_dependencies: [] },
    });

    await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');

    expect(mocks.modsInstallWithDeps).toHaveBeenCalledWith(
      'inst-1',
      { source: 'modrinth', project_id: 'core-id', version_id: 'v-new' },
      [],
    );
  });

  it('returns { ok: true } when both commands succeed', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Core', installed_dependencies: [] },
    });

    const result = await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');

    expect(result).toEqual({ ok: true, installedVersion: '1.3.52' });
  });

  it('returns { ok: false, reason: "update-failed" } when the install/update command errors', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({ status: 'error', error: 'disk full' });

    const result = await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');

    expect(result).toEqual({ ok: false, reason: 'update-failed' });
  });

  it('updates in place via modsUpdateOne when provider_sha1 is present', async () => {
    vi.mocked(mocks.modsVersions).mockResolvedValue({ status: 'ok', data: [fakeVersion] } as any);
    vi.mocked(mocks.modsUpdateOne).mockResolvedValue({ status: 'ok', data: null } as any);
    const v = { ...modrinthViolation, provider_sha1: 'OLDSHA' };
    const r = await remediateViolation('inst', v as any, '1.20.1', 'forge');
    expect(r.ok).toBe(true);
    expect(mocks.modsUpdateOne).toHaveBeenCalledWith('inst', 'OLDSHA', fakeVersion);
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('falls back to install when provider_sha1 is absent', async () => {
    vi.mocked(mocks.modsVersions).mockResolvedValue({ status: 'ok', data: [fakeVersion] } as any);
    vi.mocked(mocks.modsInstallWithDeps).mockResolvedValue({ status: 'ok', data: {} } as any);
    const v = { ...modrinthViolation, provider_sha1: null };
    const r = await remediateViolation('inst', v as any, '1.20.1', 'forge');
    expect(r.ok).toBe(true);
    expect(mocks.modsUpdateOne).not.toHaveBeenCalled();
    expect(mocks.modsInstallWithDeps).toHaveBeenCalled();
  });

  it('picks the newest SATISFYING version (not data[0]) and reports it', async () => {
    // data[0] is a snapshot beta (MC-compatible but out of range); only the
    // third entry satisfies the declared range.
    const beta = { ...fakeVersion, version_id: 'vBeta', version_number: '0.9.0-beta.1' };
    const v6 = { ...fakeVersion, version_id: 'v6', version_number: '0.6.0' };
    const v511 = { ...fakeVersion, version_id: 'v511', version_number: '0.5.11' };
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [beta, v6, v511] });
    mocks.modsFilterSatisfying.mockResolvedValue([2]);
    mocks.modsUpdateOne.mockResolvedValue({ status: 'ok', data: null });

    const v = {
      ...modrinthViolation,
      needed: '0.5.11',
      family: 'fabric_predicate' as const,
      provider_sha1: 'old',
    };
    const r = await remediateViolation('inst', v, '1.21', 'fabric');

    expect(mocks.modsFilterSatisfying).toHaveBeenCalledWith(
      ['0.9.0-beta.1', '0.6.0', '0.5.11'],
      '0.5.11',
      'fabric_predicate',
    );
    expect(mocks.modsUpdateOne).toHaveBeenCalledWith('inst', 'old', v511);
    expect(r).toEqual({ ok: true, installedVersion: '0.5.11' });
  });

  it('returns { ok: false, reason: "no-satisfying" } when nothing fits the range', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsFilterSatisfying.mockResolvedValue([]);
    const v = { ...modrinthViolation, provider_sha1: 'old' };
    const r = await remediateViolation('inst', v, '1.21', 'fabric');
    expect(r).toEqual({ ok: false, reason: 'no-satisfying' });
    expect(mocks.modsUpdateOne).not.toHaveBeenCalled();
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('returns no-provider when the violation has no family', async () => {
    const v = { ...modrinthViolation, family: null };
    const r = await remediateViolation('inst', v, '1.21', 'fabric');
    expect(r).toEqual({ ok: false, reason: 'no-provider' });
    expect(mocks.modsVersions).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// remediatePickedVersion + violationKey
// ---------------------------------------------------------------------------

describe('remediatePickedVersion + violationKey', () => {
  beforeEach(() => {
    mocks.modsInstallWithDeps.mockReset();
    mocks.modsUpdateOne.mockReset();
  });

  it('violationKey is `${dependent_sha1}:${dep_id}`', () => {
    expect(violationKey(modrinthViolation)).toBe('aa:sophisticatedcore');
  });

  it('installs the chosen version in place via modsUpdateOne when provider_sha1 is set', async () => {
    mocks.modsUpdateOne.mockResolvedValue({ status: 'ok', data: null });
    const chosen = { ...fakeVersion, version_id: 'vChosen', version_number: '0.6.0' };
    const v = { ...modrinthViolation, provider_sha1: 'OLD' };
    const r = await remediatePickedVersion('inst', v, chosen);
    expect(mocks.modsUpdateOne).toHaveBeenCalledWith('inst', 'OLD', chosen);
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
    expect(r).toEqual({ ok: true, installedVersion: '0.6.0' });
  });

  it('falls back to install when provider_sha1 is absent and reports failure honestly', async () => {
    mocks.modsInstallWithDeps.mockResolvedValue({ status: 'error', error: 'disk full' });
    const chosen = { ...fakeVersion, version_id: 'vChosen', version_number: '0.6.0' };
    const v = { ...modrinthViolation, provider_sha1: null };
    const r = await remediatePickedVersion('inst', v, chosen);
    expect(mocks.modsInstallWithDeps).toHaveBeenCalled();
    expect(mocks.modsUpdateOne).not.toHaveBeenCalled();
    expect(r).toEqual({ ok: false });
  });
});

// ---------------------------------------------------------------------------
// remediateAll
// ---------------------------------------------------------------------------

describe('remediateAll', () => {
  beforeEach(() => {
    mocks.modsVersions.mockReset();
    mocks.modsFilterSatisfying.mockReset();
    mocks.modsInstallWithDeps.mockReset();
    mocks.modsFilterSatisfying.mockResolvedValue([0]);
  });

  it('returns 0 when report has no version_out_of_range violations with a provider', async () => {
    const report: PreflightReport = { violations: [missingViolation, noProviderViolation] };
    const count = await remediateAll('inst-1', report, '1.20.1', 'fabric');
    expect(count).toBe(0);
    expect(mocks.modsVersions).not.toHaveBeenCalled();
  });

  it('returns the count of successfully updated violations', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Core', installed_dependencies: [] },
    });

    const report: PreflightReport = {
      violations: [modrinthViolation, missingViolation, curseforgeViolation],
    };
    // curseforgeViolation also succeeds (modsVersions/Install return ok for any call)
    const cfVersion = { ...fakeVersion, source: 'curseforge' as const, project_id: '99999' };
    mocks.modsVersions.mockResolvedValueOnce({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValueOnce({
      status: 'ok',
      data: { primary_name: 'Core', installed_dependencies: [] },
    });
    mocks.modsVersions.mockResolvedValueOnce({ status: 'ok', data: [cfVersion] });
    mocks.modsInstallWithDeps.mockResolvedValueOnce({
      status: 'ok',
      data: { primary_name: 'CF', installed_dependencies: [] },
    });

    const count = await remediateAll('inst-1', report, '1.20.1', 'fabric');
    // missingViolation is skipped (no provider), modrinth+cf both succeed → 2
    expect(count).toBe(2);
  });

  it('returns 0 when all remediateViolation calls fail (e.g. offline / no compatible version)', async () => {
    // Both providers return an error (network offline, etc.)
    mocks.modsVersions.mockResolvedValue({ status: 'error', error: 'network failure' });

    const report: PreflightReport = {
      violations: [modrinthViolation, curseforgeViolation],
    };
    const count = await remediateAll('inst-1', report, '1.20.1', 'fabric');
    // modsVersions errors → remediateViolation returns { ok: false } for both → 0 updated
    expect(count).toBe(0);
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// decideLaunch
// ---------------------------------------------------------------------------

describe('decideLaunch', () => {
  it('returns "launch" when preflight command errors (fail-open)', () => {
    const result = decideLaunch({ status: 'error', error: 'network error' });
    expect(result).toBe('launch');
  });

  it('returns "launch" when the report has no violations', () => {
    const result = decideLaunch({ status: 'ok', data: { violations: [] } });
    expect(result).toBe('launch');
  });

  it('returns "gate" when the report has at least one violation', () => {
    const report: PreflightReport = { violations: [modrinthViolation] };
    const result = decideLaunch({ status: 'ok', data: report });
    expect(result).toBe('gate');
  });

  it('returns "gate" for a missing_required violation', () => {
    const report: PreflightReport = { violations: [missingViolation] };
    const result = decideLaunch({ status: 'ok', data: report });
    expect(result).toBe('gate');
  });
});
