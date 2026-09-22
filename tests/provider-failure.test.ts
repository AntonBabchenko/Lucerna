// INT-06: a provider's answer is described by what it means, not by "HTTP n".
import { beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import { describeProviderFailure, providerFailureOrNull } from '$lib/l10n/provider-failure';

const err = (status: number, provider = 'gemini') => ({
  kind: 'l10n_prefill_provider' as const,
  provider,
  status,
  details: 'raw body',
});

describe('describeProviderFailure', () => {
  beforeAll(() => {
    locale.set('en');
  });

  it('a 401 / 403 is a rejected key, by display name', () => {
    expect(describeProviderFailure(err(401), 'test')).toContain(
      'Google Gemini rejected the API key',
    );
    expect(describeProviderFailure(err(403, 'anthropic'), 'run')).toContain(
      'Anthropic rejected the API key',
    );
  });

  it('a 404 is a wrong model name', () => {
    expect(describeProviderFailure(err(404), 'test')).toContain("doesn't know that model name");
  });

  it('a 429 is a rate limit or quota, naming the request', () => {
    expect(describeProviderFailure(err(429), 'test')).toContain(
      'is rate-limiting the test request',
    );
    expect(describeProviderFailure(err(429), 'run')).toContain('the translation request');
  });

  it('a 5xx is provider trouble with the status', () => {
    expect(describeProviderFailure(err(503), 'test')).toContain('is having trouble (HTTP 503)');
  });

  it('status 0 is an answer Lucerna could not read', () => {
    expect(describeProviderFailure(err(0), 'test')).toContain("couldn't read");
  });

  it('anything else is a refusal with the status', () => {
    expect(describeProviderFailure(err(418), 'test')).toContain(
      'refused the test request (HTTP 418)',
    );
  });

  it('every sentence points at Logs and never renders the body', () => {
    for (const status of [401, 404, 429, 503, 0, 418]) {
      const m = describeProviderFailure(err(status), 'test');
      expect(m).toContain('The full response is in Logs.');
      expect(m).not.toContain('raw body');
    }
  });

  it('an unknown provider id is shown as-is', () => {
    expect(describeProviderFailure(err(401, 'foo'), 'test')).toContain('foo rejected');
  });

  it('is null for any other error kind', () => {
    expect(
      providerFailureOrNull({ kind: 'mods_network', url: 'u', details: 'd' }, 'test'),
    ).toBeNull();
  });
});
