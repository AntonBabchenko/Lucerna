import { describe, expect, it } from 'vitest';
import { signInNotRenewedToast } from '../src/lib/accounts/sign-in-not-renewed';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

// The launch went ahead on the saved Microsoft sign-in because it could not be
// renewed. The toast says so, and asks to sign in again only when Microsoft
// itself refused the saved sign-in: offline or Microsoft down is not fixed by
// signing in, and saying it would be a confident answer out of ignorance.

describe('the sign-in-not-renewed toast', () => {
  it('asks to sign in again when Microsoft refused the saved sign-in', () => {
    const toast = signInNotRenewedToast({
      instance_id: 'i1',
      account_name: 'Steve',
      error: { kind: 'microsoft_sign_in_expired' },
    });
    expect(toast).toEqual({
      titleKey: 'page.accounts.notRenewedExpiredTitle',
      name: 'Steve',
      cause: null,
      lineKeys: ['page.accounts.notRenewedStarted'],
      signInAgain: true,
    });
  });

  it('names the cause and offers no sign-in when Microsoft could not be reached', () => {
    const error = {
      kind: 'network',
      url: 'https://login.live.com/oauth20_token.srf',
      failure: { kind: 'unreachable' },
      details: 'dns error',
    } as const;
    const toast = signInNotRenewedToast({ instance_id: 'i1', account_name: 'Alex', error });
    expect(toast).toEqual({
      titleKey: 'page.accounts.notRenewedTitle',
      name: 'Alex',
      cause: error,
      lineKeys: ['page.accounts.notRenewedStarted', 'page.accounts.notRenewedRetry'],
      signInAgain: false,
    });
  });

  it('treats any other failure as a renewal to retry, not a sign-in to redo', () => {
    const error = { kind: 'auth_failed', stage: 'refresh', details: 'HTTP 503' } as const;
    const toast = signInNotRenewedToast({ instance_id: 'i1', account_name: 'Alex', error });
    expect(toast.signInAgain).toBe(false);
    expect(toast.cause).toEqual(error);
  });

  it('has its words in both shipped full locales', () => {
    for (const dict of [en, ru]) {
      const a = dict.page.accounts;
      for (const value of [
        a.notRenewedTitle,
        a.notRenewedExpiredTitle,
        a.notRenewedStarted,
        a.notRenewedRetry,
        a.signInAgain,
      ]) {
        expect(value.trim()).not.toBe('');
      }
      expect(a.notRenewedTitle).toContain('{name}');
      expect(a.notRenewedExpiredTitle).toContain('{name}');
    }
  });
});
