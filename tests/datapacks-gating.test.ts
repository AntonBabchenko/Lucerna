import { describe, expect, it } from 'vitest';
import { datapacksDisabledKey } from '$lib/worlds/datapacks-gating';

describe('datapacksDisabledKey', () => {
  it('returns null when not running and not busy', () => {
    expect(datapacksDisabledKey({ running: false, busy: false })).toBeNull();
  });
  it('flags running', () => {
    expect(datapacksDisabledKey({ running: true, busy: false })).toBe(
      'worlds.datapacks.blockedRunning',
    );
  });
  it('flags busy', () => {
    expect(datapacksDisabledKey({ running: false, busy: true })).toBe(
      'worlds.datapacks.blockedBusy',
    );
  });
  it('flags running first when both apply', () => {
    expect(datapacksDisabledKey({ running: true, busy: true })).toBe(
      'worlds.datapacks.blockedRunning',
    );
  });
});
