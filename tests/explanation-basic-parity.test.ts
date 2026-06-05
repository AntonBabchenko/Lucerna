import { describe, expect, it } from 'vitest';
import { STEPS_BY_ID } from '$lib/onboarding/contextual-tours';
import { STEPS } from '$lib/onboarding/steps';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

type Json = Record<string, unknown>;

function getKey(obj: Json, dotted: string): unknown {
  return dotted.split('.').reduce<unknown>((acc, part) => {
    if (acc && typeof acc === 'object') return (acc as Json)[part];
    return undefined;
  }, obj);
}

// Every base key whose copy adapts by level.
const adaptiveBases: string[] = [
  // tour steps (skip the chooser step, which has kind === 'chooser')
  ...STEPS.filter((s) => s.kind !== 'chooser').flatMap((s) => [s.titleKey, s.bodyKey]),
  // contextual steps
  ...Object.values(STEPS_BY_ID).flatMap((steps) => steps.flatMap((s) => [s.titleKey, s.bodyKey])),
  // concept tooltips (body only). NOTE: step call-sites above are covered
  // automatically from STEPS/STEPS_BY_ID, but non-step call-sites are listed by
  // hand — if you add a new `explainKey(...)` render site for a fixed key, add
  // its base key here so a missing `*Basic` sibling is caught.
  'onboarding.instanceConcept.body',
  'onboarding.modpackInstance.body',
];

describe('Basic copy siblings exist for every adaptive key', () => {
  for (const base of adaptiveBases) {
    const basicKey = `${base}Basic`;
    it(`${basicKey} is present and non-empty in en + ru`, () => {
      const e = getKey(en as Json, basicKey);
      const r = getKey(ru as Json, basicKey);
      expect(typeof e === 'string' && (e as string).trim().length > 0).toBe(true);
      expect(typeof r === 'string' && (r as string).trim().length > 0).toBe(true);
    });
  }
});
