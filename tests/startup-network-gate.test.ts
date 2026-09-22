import { describe, expect, it } from 'vitest';
import { startupDialAllowed } from '$lib/settings/startup-network';

describe('startupDialAllowed', () => {
  it('dials only when the user allowed it AND the launcher runs from its data folder', () => {
    expect(startupDialAllowed(true, true, false)).toBe(true);
  });

  it('never dials when the user turned the startup check off', () => {
    expect(startupDialAllowed(false, true, false)).toBe(false);
  });

  it("never dials in a recovery session — its settings are defaults, not the user's", () => {
    // On a throwaway root `check_updates_on_startup` reads `true` for everyone.
    expect(startupDialAllowed(true, true, true)).toBe(false);
  });

  it('never dials while it cannot tell which session this is', () => {
    expect(startupDialAllowed(true, false, false)).toBe(false);
  });
});
