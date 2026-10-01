/**
 * Tests for the remediation helper remediatePickedVersion (the manual version pick) and
 * the launch decision helper (decideLaunch) added in Task 12. A version conflict's automatic
 * fix is the two-sided planner (tests/violation-view.test.ts, tests/preflight-panel.test.ts).
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

import { decideLaunch, remediatePickedVersion, violationKey } from '$lib/mods/preflight.svelte';
import { rawRangeDesc } from './test-utils/range-desc';

// ---------------------------------------------------------------------------
// Shared violation fixtures
// ---------------------------------------------------------------------------

const modrinthViolation: DepViolation = {
  kind: 'version_out_of_range',
  dependent_name: 'Sophisticated Backpacks',
  dependent_sha1: 'aa',
  dep_id: 'sophisticatedcore',
  needed: '[1.3.51,)',
  needed_desc: rawRangeDesc('[1.3.51,)'),
  installed_version: '1.3.50',
  provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
  provider_sha1: null,
  family: 'maven',
};

const missingViolation: DepViolation = {
  kind: 'missing_required',
  dependent_name: 'Backpacks',
  dependent_sha1: 'dd',
  dep_id: 'missingmod',
  needed: '',
  needed_desc: rawRangeDesc(''),
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
    expect(mocks.modsUpdateOne).toHaveBeenCalledWith('inst', 'OLD', chosen, false);
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
    expect(r).toEqual({ ok: false, error: 'disk full' });
  });

  // 2026-09-20 spec, D5 + D6.
  it('replaces the installed build in place when the caller found one, even without provider_sha1', async () => {
    // The preflight only knows `provider_sha1` for a TRACKED provider. When the
    // view finds another build of the same project installed, the pick is still
    // a version switch, and a switch is ONE command that downloads before it
    // removes anything.
    mocks.modsUpdateOne.mockResolvedValue({ status: 'ok', data: null });
    // Answered too, so that TODAY'S code (which installs) completes and the
    // test fails on the assertion below, not on an undefined result.
    mocks.modsInstallWithDeps.mockResolvedValue({ status: 'ok', data: {} });
    const chosen = { ...fakeVersion, version_id: 'vChosen', version_number: '0.6.0' };
    const v = { ...modrinthViolation, provider_sha1: null };

    const r = await remediatePickedVersion('inst', v, chosen, { installedSha1: 'INSTALLED' });

    expect(mocks.modsUpdateOne).toHaveBeenCalledWith('inst', 'INSTALLED', chosen, false);
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
    expect(r).toEqual({ ok: true, installedVersion: '0.6.0' });
  });

  it('carries the consent to whichever command runs', async () => {
    mocks.modsUpdateOne.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsInstallWithDeps.mockResolvedValue({ status: 'ok', data: {} });
    const chosen = { ...fakeVersion, version_id: 'vChosen', version_number: '0.6.0' };

    await remediatePickedVersion('inst', { ...modrinthViolation, provider_sha1: 'OLD' }, chosen, {
      allowOffPlatform: true,
    });
    await remediatePickedVersion('inst', { ...modrinthViolation, provider_sha1: null }, chosen, {
      allowOffPlatform: true,
    });

    expect(mocks.modsUpdateOne).toHaveBeenCalledWith('inst', 'OLD', chosen, true);
    expect(mocks.modsInstallWithDeps).toHaveBeenCalledWith(
      'inst',
      { source: 'modrinth', project_id: 'core-id', version_id: 'vChosen' },
      [],
      true,
    );
  });

  it('hands the typed error back so the caller can ask instead of toasting', async () => {
    const refusal = {
      kind: 'mod_version_not_for_instance',
      version_mc: ['1.20.1'],
      version_loaders: ['fabric'],
      instance_mc: '1.21.1',
      instance_loader: 'neoforge',
    };
    mocks.modsUpdateOne.mockResolvedValue({ status: 'error', error: refusal });
    const chosen = { ...fakeVersion, version_id: 'vChosen', version_number: '0.6.0' };

    const r = await remediatePickedVersion(
      'inst',
      { ...modrinthViolation, provider_sha1: 'OLD' },
      chosen,
    );

    expect(r).toEqual({ ok: false, error: refusal });
  });
});

// ---------------------------------------------------------------------------
// decideLaunch
// ---------------------------------------------------------------------------

describe('decideLaunch', () => {
  it('reports "unknown" — NOT "launch" — when the preflight command errors', () => {
    // The whole point of the three-state result: a check that could not run and
    // a check that passed are different facts, and collapsing them is how a
    // detector comes to report "no problems" when it never ran. It still
    // launches (the caller must not block on this) — but it has to say so.
    // NB this preflight is network-free, so `unknown` means a local read
    // failure, not being offline.
    const result = decideLaunch({ status: 'error', error: 'network error' });
    expect(result).toEqual({ kind: 'unknown', error: 'network error' });
  });

  it('returns "launch" when the report has no violations', () => {
    const result = decideLaunch({ status: 'ok', data: { violations: [] } });
    expect(result).toEqual({ kind: 'launch' });
  });

  it('returns "gate" with the report when there is at least one violation', () => {
    const report: PreflightReport = { violations: [modrinthViolation] };
    const result = decideLaunch({ status: 'ok', data: report });
    // The report rides on the decision, so the caller no longer has to cast the
    // raw IPC result back to `{ status: 'ok' }` to reach it.
    expect(result).toEqual({ kind: 'gate', report });
  });

  it('returns "gate" for a missing_required violation', () => {
    const report: PreflightReport = { violations: [missingViolation] };
    const result = decideLaunch({ status: 'ok', data: report });
    expect(result).toEqual({ kind: 'gate', report });
  });
});
