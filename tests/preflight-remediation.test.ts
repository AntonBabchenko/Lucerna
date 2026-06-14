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
  modsInstallWithDeps: vi.fn(),
  instanceDependencyPreflight: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
// preflight-cache is imported by preflight.svelte; provide a no-op
vi.mock('$lib/mods/preflight-cache', () => ({
  preflightCache: { get: vi.fn(), set: vi.fn(), delete: vi.fn() },
}));

import { decideLaunch, remediateAll, remediateViolation } from '$lib/mods/preflight.svelte';

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
    mocks.modsInstallWithDeps.mockReset();
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

    expect(result).toEqual({ ok: true, reason: undefined });
  });

  it('returns { ok: false, reason: "install-failed" } when modsInstallWithDeps errors', async () => {
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [fakeVersion] });
    mocks.modsInstallWithDeps.mockResolvedValue({ status: 'error', error: 'disk full' });

    const result = await remediateViolation('inst-1', modrinthViolation, '1.20.1', 'fabric');

    expect(result).toEqual({ ok: false, reason: 'install-failed' });
  });
});

// ---------------------------------------------------------------------------
// remediateAll
// ---------------------------------------------------------------------------

describe('remediateAll', () => {
  beforeEach(() => {
    mocks.modsVersions.mockReset();
    mocks.modsInstallWithDeps.mockReset();
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
