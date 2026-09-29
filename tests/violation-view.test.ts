import { get } from 'svelte/store';
import { beforeAll, describe, expect, it } from 'vitest';
import { locale, t } from '$lib/i18n';
import type { DepViolation } from '$lib/ipc/bindings';
import {
  isFixable,
  planOffers,
  platformLabel,
  violationAction,
  violationMessage,
} from '$lib/mods/violation-view';
import { rangeDesc, rawRangeDesc } from './test-utils/range-desc';

const base: DepViolation = {
  kind: 'missing_required',
  dependent_name: 'Waystones',
  dependent_sha1: 'w',
  dep_id: 'balm',
  needed: '',
  needed_desc: rawRangeDesc(''),
  installed_version: null,
  provider_project: null,
  provider_sha1: null,
  family: null,
};

describe('violationMessage', () => {
  beforeAll(() => locale.set('en'));

  it('says a disabled provider is disabled — never "not installed"', () => {
    const v: DepViolation = { ...base, kind: 'required_disabled', provider_sha1: 'balm-sha' };
    const s = violationMessage(get(t), v, 'Balm');
    expect(s).toBe('Waystones: Balm is disabled');
    expect(s).not.toContain('not installed');
  });

  it('words a platform mismatch as what the jar is made for, with a readable platform', () => {
    const v: DepViolation = {
      ...base,
      kind: 'platform_mismatch',
      dep_id: 'minecraft',
      needed: '1.20.1',
      needed_desc: rangeDesc('1.20.1', [{ kind: 'exact', version: '1.20.1' }]),
      installed_version: '1.21.1',
      family: 'maven',
    };
    expect(violationMessage(get(t), v, 'minecraft')).toBe(
      'Waystones is made for Minecraft exactly 1.20.1 — this profile has 1.21.1',
    );
  });

  it('keeps the missing-dependency sentence', () => {
    expect(violationMessage(get(t), base, 'Balm')).toBe(
      'Waystones needs Balm, which is not installed',
    );
  });
});

describe('violationAction', () => {
  it.each([
    ['required_disabled', 'enable'],
    ['missing_required', 'install'],
    ['version_out_of_range', 'plan'],
    ['optional_out_of_range', 'plan'],
    ['incompatible_installed', 'plan'],
    ['platform_mismatch', 'none'],
  ] as const)('%s → %s', (kind, action) => {
    expect(violationAction({ ...base, kind, provider_sha1: 'p' })).toBe(action);
  });

  it('offers no Enable when the report names no disabled jar to switch on', () => {
    expect(violationAction({ ...base, kind: 'required_disabled', provider_sha1: null })).toBe(
      'none',
    );
  });

  it('never counts a platform mismatch as fixable (spec §6.4)', () => {
    expect(isFixable({ ...base, kind: 'platform_mismatch' })).toBe(false);
    expect(isFixable({ ...base, kind: 'required_disabled', provider_sha1: 'p' })).toBe(true);
  });

  it('names loader axes the way players know them', () => {
    expect(platformLabel('fabricloader')).toBe('Fabric Loader');
    expect(platformLabel('neoforge')).toBe('NeoForge');
    expect(platformLabel('somethingelse')).toBe('somethingelse');
  });
});

// The planner's two fixes as buttons (spec D8, §6.5).
describe('planOffers', () => {
  beforeAll(() => locale.set('en'));
  const v: DepViolation = {
    ...base,
    kind: 'version_out_of_range',
    dependent_name: 'Indium',
    dep_id: 'sodium',
  };
  const ver = (n: string) => ({ version_number: n }) as never;
  const provider = (direction: 'upgrade' | 'downgrade' | 'unknown', breaks: string[] = []) => ({
    update_dependent: null,
    change_provider: { version: ver('0.5.11'), direction, breaks },
  });

  it('puts «update the dependent» first and makes it the primary action', () => {
    const plan = {
      update_dependent: { version: ver('1.2'), breaks: [] },
      change_provider: provider('downgrade').change_provider,
    };
    expect(planOffers(get(t), v, plan, 'Sodium').map((o) => [o.side, o.label, o.primary])).toEqual([
      ['dependent', 'Update Indium to 1.2', true],
      ['provider', 'Roll Sodium back to 0.5.11', false],
    ]);
  });

  it('names a provider change by its real direction — neutrally when it is unknown (A-F5)', () => {
    const label = (d: 'upgrade' | 'downgrade' | 'unknown') =>
      planOffers(get(t), v, provider(d), 'Sodium')[0]?.label;
    expect(label('upgrade')).toBe('Update Sodium to 0.5.11');
    expect(label('downgrade')).toBe('Roll Sodium back to 0.5.11');
    // Never a guessed «Update» or «Roll back» where a qualifier decides the order.
    expect(label('unknown')).toBe('Switch Sodium to 0.5.11');
  });

  it('a lone provider change that breaks nothing is the primary action', () => {
    expect(planOffers(get(t), v, provider('upgrade'), 'Sodium')[0]).toMatchObject({
      side: 'provider',
      primary: true,
      breaks: [],
    });
  });

  it('never makes a breaking change the primary action, and carries whom it breaks (D8)', () => {
    const [o] = planOffers(get(t), v, provider('downgrade', ['Iris', 'Reese']), 'Sodium');
    expect(o).toMatchObject({ side: 'provider', primary: false, breaks: ['Iris', 'Reese'] });
  });

  // Updating the dependent can push it out of another mod's range on it (D8): that update is
  // no default either — a provider change that breaks nothing is.
  it('a dependent update that breaks another mod carries whom and yields the default', () => {
    const plan = {
      update_dependent: { version: ver('1.2'), breaks: ['Pin'] },
      change_provider: provider('downgrade').change_provider,
    };
    const offers = planOffers(get(t), v, plan, 'Sodium');
    expect(offers.map((o) => [o.side, o.primary, o.breaks])).toEqual([
      ['dependent', false, ['Pin']],
      ['provider', true, []],
    ]);
  });

  it('when both sides break another mod, neither is the primary action', () => {
    const plan = {
      update_dependent: { version: ver('1.2'), breaks: ['Pin'] },
      change_provider: provider('downgrade', ['Iris']).change_provider,
    };
    const offers = planOffers(get(t), v, plan, 'Sodium');
    expect(offers.map((o) => [o.side, o.primary, o.breaks])).toEqual([
      ['dependent', false, ['Pin']],
      ['provider', false, ['Iris']],
    ]);
  });

  it('offers nothing for a plan with neither side', () => {
    const none = { update_dependent: null, change_provider: null };
    expect(planOffers(get(t), v, none, 'Sodium')).toEqual([]);
  });
});
