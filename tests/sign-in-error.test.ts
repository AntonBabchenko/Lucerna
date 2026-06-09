import { describe, expect, it } from 'vitest';
import { classifySignInError, MINECRAFT_BUY_URL } from '../src/lib/accounts/sign-in-error';

describe('classifySignInError', () => {
  it('maps auth_pending_approval to an info toast with no buy link', () => {
    const result = classifySignInError({ kind: 'auth_pending_approval' });
    expect(result.kind).toBe('info');
    expect(result.titleKey).toBe('page.accounts.pendingApproval');
    expect(result.buyLink).toBeUndefined();
  });

  it('maps no_minecraft_profile to a warning toast carrying the buy link', () => {
    const result = classifySignInError({ kind: 'no_minecraft_profile' });
    expect(result.kind).toBe('warning');
    expect(result.titleKey).toBe('page.accounts.noMinecraftTitle');
    expect(result.buyLink).toBe(MINECRAFT_BUY_URL);
  });

  it('falls back to a generic warning for unknown errors and no buy link', () => {
    expect(classifySignInError({ kind: 'auth_failed' })).toMatchObject({
      kind: 'warning',
      titleKey: 'page.accounts.signInFailed',
    });
    expect(classifySignInError({ kind: 'auth_failed' }).buyLink).toBeUndefined();
    // Non-object / missing kind must not throw and stays generic.
    expect(classifySignInError(null).titleKey).toBe('page.accounts.signInFailed');
    expect(classifySignInError(undefined).buyLink).toBeUndefined();
  });

  it('points the buy link at the official Minecraft store page', () => {
    expect(MINECRAFT_BUY_URL).toMatch(/^https:\/\/www\.minecraft\.net\//);
  });
});
