// The rule behind the translate buttons' enablement. It is a second, advisory
// expression of `AiProvider::needs_key()` (Rust), so what is worth pinning is
// exactly the places the two could drift apart: which providers need a key,
// and which question is asked first.

import { describe, expect, it } from 'vitest';
import { needsStoredKey, prefillReadiness } from '$lib/l10n/prefill-readiness';

describe('prefillReadiness', () => {
  it('asks about consent before it asks about a credential', () => {
    // Both are missing. Telling a user to paste an API key for a feature they
    // have not permitted sends them to do work that changes nothing — the
    // permission is the outer gate and has to be named first.
    expect(prefillReadiness({ consent: false, provider: 'anthropic', keyStored: false })).toBe(
      'no_consent',
    );
    expect(prefillReadiness({ consent: false, provider: 'local', keyStored: true })).toBe(
      'no_consent',
    );
  });

  it('requires a stored key from every hosted provider', () => {
    for (const provider of ['anthropic', 'gemini', 'groq'] as const) {
      expect(needsStoredKey(provider)).toBe(true);
      expect(prefillReadiness({ consent: true, provider, keyStored: false })).toBe('no_key');
      expect(prefillReadiness({ consent: true, provider, keyStored: true })).toBe('ready');
    }
  });

  it('never asks a local server for a credential it has no way to want', () => {
    // Mirrors `AiProvider::needs_key()` returning false for Local. Getting
    // this wrong would park a permanently dead button in front of every
    // local-model user, with a tooltip telling them to buy an API key.
    expect(needsStoredKey('local')).toBe(false);
    expect(prefillReadiness({ consent: true, provider: 'local', keyStored: false })).toBe('ready');
  });
});
