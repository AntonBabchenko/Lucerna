import { describe, it, expect } from 'vitest';
import { quickPlayDisabledKey } from '$lib/worlds/quick-play-gating';

describe('quickPlayDisabledKey', () => {
  it('returns null when ready, not running, supported', () => {
    expect(quickPlayDisabledKey({ ready: true, running: false, supported: true })).toBeNull();
  });
  it('flags not-ready first', () => {
    expect(quickPlayDisabledKey({ ready: false, running: false, supported: true })).toBe(
      'worlds.quickPlay.disabledNotReady',
    );
  });
  it('flags running', () => {
    expect(quickPlayDisabledKey({ ready: true, running: true, supported: true })).toBe(
      'worlds.quickPlay.disabledRunning',
    );
  });
  it('flags unsupported version', () => {
    expect(quickPlayDisabledKey({ ready: true, running: false, supported: false })).toBe(
      'worlds.quickPlay.disabledUnsupported',
    );
  });
});
