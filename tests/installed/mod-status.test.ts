/**
 * One status per installed mod (spec 2026-09-28 §6.2): blocking from the pre-flight only
 * (platform mismatch included), warnings from compat only, `disabled` for what the loader never
 * reads. The «Проблемы» chip, the row and the Overview share it.
 */
import { describe, expect, it } from 'vitest';
import type { DepViolation, ModUpdateState, ViolationKind } from '$lib/ipc/bindings';
import { isProblem, type StatusInput, statusOf } from '$lib/mods/installed/mod-status';
import { rawRangeDesc } from '../test-utils/range-desc';

const v = (kind: ViolationKind, depId = 'dep'): DepViolation => ({
  dependent_sha1: 'a',
  dependent_name: 'Alpha',
  dep_id: depId,
  kind,
  installed_version: null,
  needed: '',
  needed_desc: rawRangeDesc(''),
  provider_project: null,
  provider_sha1: null,
  family: null,
});
const available = { kind: 'update_available', target: {} } as unknown as ModUpdateState;
const input = (over: Partial<StatusInput> = {}): StatusInput => ({
  enabled: true,
  violations: [],
  compat: null,
  update: null,
  held: false,
  ...over,
});

describe('statusOf', () => {
  it('an enabled mod with nothing to say is ok', () => {
    expect(statusOf(input())).toEqual({ level: 'ok', reasons: [], fix: null });
  });

  it('a disabled mod is only disabled — the pre-flight does not judge it', () => {
    const s = statusOf(
      input({
        enabled: false,
        violations: [v('missing_required')],
        compat: { key: 'loader', detected: 'Fabric' },
        update: available,
      }),
    );
    expect(s).toEqual({ level: 'disabled', reasons: [], fix: null });
  });

  it.each([
    ['missing_required', 'install-dependency'],
    ['required_disabled', 'enable-dependency'],
    ['version_out_of_range', 'fix-version'],
    ['optional_out_of_range', 'fix-version'],
    ['incompatible_installed', 'fix-version'],
    ['platform_mismatch', 'choose-version'],
  ] as const)('a %s violation blocks, fixed by %s', (kind, fix) => {
    const violation = v(kind);
    const s = statusOf(input({ violations: [violation] }));
    expect(s.level).toBe('blocking');
    expect(s.reasons).toEqual([{ source: 'preflight', violation }]);
    expect(s.fix).toEqual({ kind: fix, violation });
  });

  it('platform hints from compat are ignored: the pre-flight is the one source for that fact', () => {
    expect(statusOf(input({ compat: { key: 'platformMc', declared: '1.20' } })).level).toBe('ok');
    expect(statusOf(input({ compat: { key: 'platformLoader', declared: '0.15' } })).level).toBe(
      'ok',
    );
  });

  it('a foreign-loader jar or a live «no release» is a warning with the migration fix', () => {
    for (const compat of [{ key: 'loader', detected: 'Fabric' }, { key: 'noRelease' }] as const) {
      const s = statusOf(input({ compat }));
      expect(s.level).toBe('warning');
      expect(s.reasons).toEqual([{ source: 'compat', hint: compat }]);
      expect(s.fix).toEqual({ kind: 'migrate' });
    }
  });

  it('orders several reasons: the jar itself first, then missing, disabled, range, clash, then compat', () => {
    const s = statusOf(
      input({
        violations: [
          v('incompatible_installed', 'z'),
          v('version_out_of_range', 'y'),
          v('missing_required', 'b'),
          v('required_disabled', 'a'),
          v('missing_required', 'a'),
          v('platform_mismatch', 'minecraft'),
        ],
        compat: { key: 'noRelease' },
      }),
    );
    expect(
      s.reasons.map((r) =>
        r.source === 'preflight' ? `${r.violation.kind}:${r.violation.dep_id}` : r.source,
      ),
    ).toEqual([
      'platform_mismatch:minecraft',
      'missing_required:a',
      'missing_required:b',
      'required_disabled:a',
      'version_out_of_range:y',
      'incompatible_installed:z',
      'compat',
    ]);
    expect(s.fix?.kind).toBe('choose-version'); // the first reason's fix
  });

  it('a self-completing pack makes pre-flight reasons advisory: a warning, not "won\'t start"', () => {
    const s = statusOf(input({ violations: [v('missing_required')], advisory: true }));
    expect(s.level).toBe('warning');
    expect(s.reasons).toHaveLength(1);
    expect(s.fix?.kind).toBe('install-dependency');
  });

  it('a hold hides an available update; otherwise an update is its own level', () => {
    expect(statusOf(input({ update: available, held: true })).level).toBe('held');
    expect(statusOf(input({ update: available }))).toEqual({
      level: 'update',
      reasons: [],
      fix: { kind: 'update' },
    });
    expect(statusOf(input({ held: true, violations: [v('missing_required')] })).level).toBe(
      'blocking',
    );
  });

  it('an update check that failed or found nothing newer is not an update', () => {
    for (const update of [
      { kind: 'check_failed', reason: 'offline' },
      { kind: 'unknown' },
      { kind: 'up_to_date' },
    ] as ModUpdateState[]) {
      expect(statusOf(input({ update })).level).toBe('ok');
    }
  });

  it("does not reorder the caller's violations in place", () => {
    const violations = Object.freeze([v('missing_required', 'b'), v('platform_mismatch')]);
    expect(() => statusOf(input({ violations }))).not.toThrow();
  });
});

describe('isProblem', () => {
  it('counts exactly blocking and warning — the «Проблемы» chip', () => {
    expect(isProblem(statusOf(input({ violations: [v('missing_required')] })))).toBe(true);
    expect(isProblem(statusOf(input({ compat: { key: 'noRelease' } })))).toBe(true);
    expect(isProblem(statusOf(input({ update: available })))).toBe(false);
    expect(isProblem(statusOf(input({ held: true })))).toBe(false);
    expect(
      isProblem(statusOf(input({ enabled: false, violations: [v('missing_required')] }))),
    ).toBe(false);
    expect(isProblem(undefined)).toBe(false);
  });
});
