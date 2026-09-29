/**
 * `fixAll` — the ONE repair behind the panel's «Fix all» and the Play gate's «Fix and launch»
 * (spec D5, §6.4) — and the gate's loop around it, `repairForLaunch`: fix, re-run the pre-flight,
 * launch only when it is clean.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  modsInstallDependency: vi.fn(),
  modsInstallMissingRequired: vi.fn(),
  modsPlanVersionFix: vi.fn(),
  updateMod: vi.fn(),
  enableModsUnguarded: vi.fn(),
  depProjectOf: vi.fn(),
  depNameOf: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsInstallDependency: m.modsInstallDependency,
    modsInstallMissingRequired: m.modsInstallMissingRequired,
    modsPlanVersionFix: m.modsPlanVersionFix,
  },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/mods/preflight-cache', () => ({
  preflightCache: { get: vi.fn(), set: vi.fn(), delete: vi.fn() },
}));
vi.mock('$lib/tasks/adapters/mod-install', () => ({
  updateMod: m.updateMod,
  installModWithDeps: vi.fn(),
}));
vi.mock('$lib/mods/mod-ops.svelte', () => ({
  enableModsUnguarded: m.enableModsUnguarded,
  // A refused mod write reads as a busy profile (plan A9); anything else by its kind here.
  modWriteReason: (e: { kind: string }) => (e.kind === 'instance_busy' ? 'BUSY' : e.kind),
}));
vi.mock('$lib/mods/dep-names.svelte', () => ({
  depProjectOf: m.depProjectOf,
  depNameOf: m.depNameOf,
}));

import { countFixed, fixAll, repairForLaunch } from '$lib/mods/fix-all';
import { rawRangeDesc } from './test-utils/range-desc';

const v = (over: Partial<DepViolation>): DepViolation => ({
  kind: 'missing_required',
  dependent_name: 'Alpha',
  dependent_sha1: 'a',
  dep_id: 'balm',
  needed: '',
  needed_desc: rawRangeDesc(''),
  installed_version: null,
  provider_project: null,
  provider_sha1: null,
  family: null,
  ...over,
});
const report = (...violations: DepViolation[]): PreflightReport => ({ violations });
// A build the planner offers; the task is named after it, like every other update.
const ver = (n: string) => ({ name: `Build ${n}`, version_number: n }) as never;
const ok = <T>(data: T) => ({ status: 'ok' as const, data });
const err = (kind: string) => ({ status: 'error' as const, error: { kind, name: 'Balm' } });

beforeEach(() => {
  for (const f of Object.values(m)) f.mockReset();
  m.depProjectOf.mockReturnValue(null);
  m.depNameOf.mockReturnValue(null);
  m.updateMod.mockResolvedValue(ok({}));
  m.enableModsUnguarded.mockResolvedValue({ enabled: [], failed: [], reasons: [] });
});

describe('fixAll', () => {
  it('switches every disabled provider on in ONE unguarded call, each jar once', async () => {
    const a = v({ kind: 'required_disabled', provider_sha1: 'p1' });
    const b = v({ kind: 'required_disabled', provider_sha1: 'p1', dependent_sha1: 'b' });
    const c = v({ kind: 'required_disabled', provider_sha1: 'p2', dep_id: 'cloth' });
    m.enableModsUnguarded.mockResolvedValue({
      enabled: ['p1', 'lib'],
      failed: ['p2'],
      reasons: ['denied'],
    });
    const out = await fixAll('i', report(a, b, c));
    expect(m.enableModsUnguarded.mock.calls).toEqual([['i', ['p1', 'p2']]]);
    expect(out).toEqual({ attempted: [a, b, c], applied: [a, b], reasons: ['denied'] });
  });

  it('installs by project when the name store knows it, else by mod-id', async () => {
    m.depProjectOf.mockImplementation((_i: string, _s: string, dep: string) =>
      dep === 'balm' ? { source: 'modrinth', project_id: 'PB' } : null,
    );
    m.modsInstallDependency.mockResolvedValue(ok({}));
    m.modsInstallMissingRequired.mockResolvedValue(ok({ kind: 'installed', name: 'Cloth' }));
    const balm = v({});
    const cloth = v({ dep_id: 'cloth' });
    const out = await fixAll('i', report(balm, cloth));
    expect(m.modsInstallDependency).toHaveBeenCalledWith('i', 'a', 'modrinth', 'PB');
    expect(m.modsInstallMissingRequired).toHaveBeenCalledWith('i', 'a', 'cloth');
    expect(out.applied).toEqual([balm, cloth]);
  });

  it('installs a dependency several mods miss once — a second jar would crash the game', async () => {
    m.modsInstallMissingRequired.mockResolvedValue(ok({ kind: 'installed', name: 'Balm' }));
    const a = v({});
    const b = v({ dependent_sha1: 'b', dependent_name: 'Beta' });
    const out = await fixAll('i', report(a, b));
    expect(m.modsInstallMissingRequired).toHaveBeenCalledTimes(1);
    expect(out.applied).toEqual([a, b]);
  });

  it('a dependency the profile already lists is there already — never a failure', async () => {
    // An earlier step of this run brought it under another mod-id, or the user did meanwhile.
    m.depProjectOf.mockImplementation((_i: string, _s: string, dep: string) =>
      dep === 'balm' ? { source: 'modrinth', project_id: 'PB' } : null,
    );
    m.modsInstallDependency.mockResolvedValue(err('mods_already_installed'));
    m.modsInstallMissingRequired.mockResolvedValue(err('mods_already_installed'));
    const balm = v({});
    const cloth = v({ dep_id: 'cloth' });
    const out = await fixAll('i', report(balm, cloth));
    expect(out.applied).toEqual([balm, cloth]);
  });

  it('an install that found nothing, or failed, is not applied', async () => {
    m.depProjectOf.mockImplementation((_i: string, _s: string, dep: string) =>
      dep === 'balm' ? { source: 'modrinth', project_id: 'PB' } : null,
    );
    m.modsInstallDependency.mockResolvedValue(err('mods_dependency_unresolvable'));
    m.modsInstallMissingRequired.mockResolvedValue(ok({ kind: 'open_search', query: 'cloth' }));
    const out = await fixAll('i', report(v({}), v({ dep_id: 'cloth' })));
    expect(out.attempted).toHaveLength(2);
    expect(out.applied).toEqual([]);
  });

  it('prefers updating the dependent; changes the provider only when nothing breaks', async () => {
    const dep = v({ kind: 'version_out_of_range', dep_id: 'sodium', dependent_name: 'Indium' });
    const breaks = v({
      kind: 'version_out_of_range',
      dep_id: 'sodium',
      dependent_sha1: 'b',
      provider_sha1: 's',
    });
    const clean = v({
      kind: 'optional_out_of_range',
      dep_id: 'curios',
      dependent_sha1: 'c',
      provider_sha1: 'cu',
    });
    m.modsPlanVersionFix.mockImplementation(async (_i: string, sha: string) =>
      sha === 'a'
        ? ok({ update_dependent: { version: ver('2.0') }, change_provider: null })
        : sha === 'b'
          ? ok({
              update_dependent: null,
              change_provider: { version: ver('0.5'), direction: 'downgrade', breaks: ['Z'] },
            })
          : ok({
              update_dependent: null,
              change_provider: { version: ver('9.1'), direction: 'upgrade', breaks: [] },
            }),
    );
    const out = await fixAll('i', report(dep, breaks, clean));
    expect(m.updateMod.mock.calls).toEqual([
      ['i', 'Build 2.0', 'a', ver('2.0')],
      ['i', 'Build 9.1', 'cu', ver('9.1')],
    ]);
    expect(out.applied).toEqual([dep, clean]);
  });

  it('never plans against a jar this run already switched — the re-check judges the new one', async () => {
    const first = v({ kind: 'version_out_of_range', dep_id: 'sodium', provider_sha1: 's' });
    const second = v({ kind: 'version_out_of_range', dep_id: 'iris', provider_sha1: 'ir' });
    m.modsPlanVersionFix.mockResolvedValue(
      ok({ update_dependent: { version: ver('2.0') }, change_provider: null }),
    );
    const out = await fixAll('i', report(first, second));
    expect(m.modsPlanVersionFix).toHaveBeenCalledTimes(1);
    expect(out).toEqual({ attempted: [first, second], applied: [first], reasons: [] });
  });

  it('a step whose bridge call throws is not applied, and the rest still run', async () => {
    m.modsInstallMissingRequired.mockRejectedValue(new Error('bridge gone'));
    m.modsPlanVersionFix.mockResolvedValue(
      ok({ update_dependent: { version: ver('2.0') }, change_provider: null }),
    );
    const plan = v({ kind: 'version_out_of_range', dep_id: 'sodium', dependent_sha1: 'b' });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const out = await fixAll('i', report(v({}), plan));
      expect(out.applied).toEqual([plan]);
    } finally {
      warn.mockRestore();
    }
  });

  it('leaves a platform mismatch alone', async () => {
    const out = await fixAll('i', report(v({ kind: 'platform_mismatch', dep_id: 'minecraft' })));
    expect(out).toEqual({ attempted: [], applied: [], reasons: [] });
    expect(m.modsPlanVersionFix).not.toHaveBeenCalled();
  });

  it('repairs nothing while a self-completing pack still fetches its own files', async () => {
    const out = await fixAll('i', {
      violations: [v({})],
      pack_completion: {
        total: 2,
        outstanding: [
          { display_name: 'Balm', pattern: 'balm-*.jar', url: null, destination: 'mods' },
        ],
      },
    });
    expect(out).toEqual({ attempted: [], applied: [], reasons: [] });
    expect(m.modsInstallMissingRequired).not.toHaveBeenCalled();
  });
});

// «Fixed 0 of N» alone cannot tell a held profile from unrelated failures: the repair says why
// each step that failed failed, each reason once, worded for the user.
describe('fixAll — why a step failed', () => {
  it('says why the steps that failed failed — each reason once, a busy profile as busy', async () => {
    m.enableModsUnguarded.mockResolvedValue({ enabled: [], failed: ['p'], reasons: ['BUSY'] });
    m.depProjectOf.mockImplementation((_i: string, _s: string, dep: string) =>
      dep === 'balm' ? { source: 'modrinth', project_id: 'PB' } : null,
    );
    m.depNameOf.mockImplementation((_i: string, _s: string, dep: string) =>
      dep === 'cloth' ? 'Cloth Config' : null,
    );
    m.modsInstallDependency.mockResolvedValue(err('instance_busy'));
    m.modsInstallMissingRequired.mockResolvedValue(ok({ kind: 'open_search', query: 'cloth' }));
    m.modsPlanVersionFix.mockImplementation(async (_i: string, sha: string) =>
      sha === 'd'
        ? err('mods_network')
        : ok({ update_dependent: { version: ver('2.0') }, change_provider: null }),
    );
    m.updateMod.mockResolvedValue(err('io'));
    const out = await fixAll(
      'i',
      report(
        v({ kind: 'required_disabled', provider_sha1: 'p', dep_id: 'lib' }),
        v({}), // Balm, by project: the profile is busy
        v({ dep_id: 'cloth' }), // by mod-id: no project provides it with confidence
        v({ kind: 'version_out_of_range', dep_id: 'sodium', dependent_sha1: 'd' }), // no plan
        v({ kind: 'version_out_of_range', dep_id: 'iris', dependent_sha1: 'e' }), // switch failed
      ),
    );
    expect(out.applied).toEqual([]);
    expect(out.reasons).toEqual([
      'BUSY',
      "Couldn't find Cloth Config automatically",
      'mods_network',
      'io',
    ]);
  });

  it('gives no reason for what is no failure: a dependency already there, a switch it may not take', async () => {
    m.modsInstallMissingRequired.mockResolvedValue(err('mods_already_installed'));
    m.modsPlanVersionFix.mockResolvedValue(
      ok({
        update_dependent: null,
        change_provider: { version: ver('0.5'), direction: 'downgrade', breaks: ['Z'] },
      }),
    );
    const plan = v({ kind: 'version_out_of_range', dep_id: 'sodium', dependent_sha1: 'b' });
    const out = await fixAll('i', report(v({}), { ...plan, provider_sha1: 's' }));
    expect(out.reasons).toEqual([]);
  });

  it('a step whose bridge call throws says why too', async () => {
    m.modsInstallMissingRequired.mockRejectedValue(new Error('bridge gone'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const out = await fixAll('i', report(v({})));
      expect(out.reasons).toEqual(['bridge gone']);
    } finally {
      warn.mockRestore();
    }
  });
});

describe('repairForLaunch', () => {
  const a = v({});
  const b = v({ kind: 'required_disabled', dep_id: 'x', provider_sha1: 'x' });
  beforeEach(() => {
    m.modsInstallMissingRequired.mockResolvedValue(ok({ kind: 'installed', name: 'Balm' }));
    m.enableModsUnguarded.mockResolvedValue({ enabled: [], failed: ['x'], reasons: ['denied'] });
  });

  it('launches when the re-check comes back clean', async () => {
    const out = await repairForLaunch('i', report(a, b), async () => ok(report()));
    expect(out).toEqual({ kind: 'launch', checked: true });
  });

  it('stays on the remaining rows with «N of M» when the repair was partial', async () => {
    const out = await repairForLaunch('i', report(a, b), async () => ok(report(b)));
    // …and says why the rest failed.
    expect(out).toEqual({
      kind: 'stay',
      report: report(b),
      fixed: 1,
      total: 2,
      reasons: ['denied'],
    });
  });

  it('launches unchecked when the re-check itself cannot run (a failed check never blocks)', async () => {
    const out = await repairForLaunch('i', report(a, b), async () => ({
      status: 'error' as const,
      error: 'io',
    }));
    expect(out).toEqual({ kind: 'launch', checked: false });
  });
});

describe('countFixed', () => {
  it('counts a row fixed only when the re-check no longer reports it', () => {
    const a = v({});
    const b = v({ dep_id: 'x' });
    expect(countFixed([a, b], report(b))).toBe(1);
  });

  it('a mod switched to another build still has its problem when the new build does', () => {
    // Updating Alpha changes its jar (and digest), not the dependency it misses.
    const before = v({ kind: 'version_out_of_range', dep_id: 'iris' });
    const after = { ...before, dependent_sha1: 'a2' };
    expect(countFixed([before], report(after))).toBe(0);
  });
});
