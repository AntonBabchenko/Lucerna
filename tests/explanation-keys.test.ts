import { describe, expect, it } from 'vitest';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import { explainKey } from '$lib/onboarding/explanation-keys';

const BASE = 'onboarding.tour.welcome.body' as TranslationKey;

describe('explainKey', () => {
  it('returns the base key unchanged for advanced', () => {
    expect(explainKey(BASE, 'advanced')).toBe('onboarding.tour.welcome.body');
  });

  it('appends Basic to the leaf for basic', () => {
    expect(explainKey(BASE, 'basic')).toBe('onboarding.tour.welcome.bodyBasic');
  });
});
