import { get } from 'svelte/store';
import { beforeAll, describe, expect, it } from 'vitest';
import { locale, t } from '$lib/i18n';
import type { DepViolation } from '$lib/ipc/bindings';
import {
  isFixable,
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
