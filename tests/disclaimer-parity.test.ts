// Drift guard: the en.json disclaimer translation must stay byte-identical to
// the canonical DISCLAIMER_TEXT constant in disclaimer.ts. This ensures the
// i18n key and the constant (used by tests and referenced as source-of-truth)
// can never silently diverge.
import { describe, expect, it } from 'vitest';
import { DISCLAIMER_TEXT } from '$lib/settings/disclaimer';
import en from '../src/lib/i18n/locales/en.json';

describe('disclaimer parity', () => {
  it('en.json settings.about.disclaimer matches the canonical DISCLAIMER_TEXT constant', () => {
    expect(en.settings.about.disclaimer).toBe(DISCLAIMER_TEXT);
  });
});
