import { get } from 'svelte/store';
import { beforeAll, describe, expect, it } from 'vitest';
import { locale, t } from '$lib/i18n';
import type { DepViolation, ModUpdateState } from '$lib/ipc/bindings';
import { type StatusInput, statusOf } from '$lib/mods/installed/mod-status';
import { type RowProblemInput, rowProblemOf } from '$lib/mods/installed/row-problem';
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

// The row line is built from the row's ONE status (mod-status.ts), so its reason and its fix are
// the ones the status ranked first — never a second ranking of its own.
const status = (over: Partial<StatusInput> = {}) =>
  statusOf({ enabled: true, violations: [], compat: null, update: null, held: false, ...over });

const input = (over: Partial<RowProblemInput> = {}): RowProblemInput => ({
  t: get(t),
  depName: (x) => (x.dep_id === 'balm' ? 'Balm' : x.dep_id),
  loader: 'neoforge',
  mc: '1.21.1',
  canChooseVersion: true,
  ...over,
});

const available = { kind: 'update_available', target: {} } as unknown as ModUpdateState;

describe('rowProblemOf', () => {
  beforeAll(() => locale.set('en'));

  it('is null for a row the status calls clean, disabled, updatable or held', () => {
    for (const s of [
      status(),
      status({ enabled: false, violations: [v({})] }),
      status({ update: available }),
      status({ update: available, held: true }),
    ])
      expect(rowProblemOf(s, input())).toBeNull();
  });

  it('leads with the reason the status ranks first — the one its fix repairs — and counts the rest', () => {
    // Report order puts the disabled provider first; the status ranks "missing" above it.
    const p = rowProblemOf(
      status({
        violations: [v({ kind: 'required_disabled', dep_id: 'x', provider_sha1: 'x' }), v({})],
      }),
      input(),
    );
    expect(p).toMatchObject({
      level: 'blocking',
      text: 'Alpha needs Balm, which is not installed',
      tooltip: null,
      more: 1,
    });
    expect(p?.fix).toMatchObject({ kind: 'install', label: 'Install Balm' });
  });

  it('offers Enable for a disabled provider and Fix… for a version conflict', () => {
    const disabled = rowProblemOf(
      status({ violations: [v({ kind: 'required_disabled', provider_sha1: 'p' })] }),
      input(),
    );
    expect(disabled?.text).toBe('Alpha: Balm is disabled');
    expect(disabled?.fix).toMatchObject({ kind: 'enable', label: 'Enable' });
    const range = rowProblemOf(
      status({ violations: [v({ kind: 'version_out_of_range', installed_version: '1' })] }),
      input(),
    );
    expect(range?.fix).toMatchObject({ kind: 'plan', label: 'Fix…' });
  });

  it('offers no Enable when the report names no disabled jar', () => {
    const p = rowProblemOf(
      status({ violations: [v({ kind: 'required_disabled', provider_sha1: null })] }),
      input(),
    );
    expect(p?.level).toBe('blocking');
    expect(p?.fix).toBeNull();
  });

  it("offers the mod's own versions for a platform mismatch — only when it has a platform identity", () => {
    const pm = v({ kind: 'platform_mismatch', dep_id: 'minecraft', installed_version: '1.21.1' });
    expect(rowProblemOf(status({ violations: [pm] }), input())?.fix).toMatchObject({
      kind: 'choose_version',
      label: 'Choose version',
    });
    expect(
      rowProblemOf(status({ violations: [pm] }), input({ canChooseVersion: false }))?.fix,
    ).toBeNull();
  });

  it('says a foreign-loader jar will not load, keeping the long hint as the tooltip', () => {
    const p = rowProblemOf(status({ compat: { key: 'loader', detected: 'Fabric' } }), input());
    expect(p).toMatchObject({
      level: 'warning',
      text: "Won't load: a mod for Fabric",
      tooltip: 'This mod is for Fabric; the instance uses NeoForge',
      more: 0,
      fix: { kind: 'choose_version' },
    });
  });

  it('says a mod with no release may not work', () => {
    const p = rowProblemOf(status({ compat: { key: 'noRelease' } }), input());
    expect(p?.text).toBe('May not work: no release for NeoForge 1.21.1');
    expect(p?.tooltip).toContain('lists no release for NeoForge 1.21.1');
  });

  it('counts only the reasons the panel lists — a compat warning behind a blocking one is not one', () => {
    const p = rowProblemOf(
      status({ violations: [v({})], compat: { key: 'loader', detected: 'Fabric' } }),
      input(),
    );
    expect(p).toMatchObject({ level: 'blocking', more: 0 });
  });
});
