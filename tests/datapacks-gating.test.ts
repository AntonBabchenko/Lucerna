import { describe, expect, it } from 'vitest';
import type { WorldPackState } from '$lib/ipc/bindings';
import { datapacksDisabledKey, worldRowKind } from '$lib/worlds/datapacks-gating';

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

describe('worldRowKind', () => {
  const row = (state: WorldPackState, in_library = true) => ({ state, in_library });
  it('classifies ghost/addable/live/ignored', () => {
    expect(worldRowKind(row('ignored'))).toBe('ignored');
    expect(worldRowKind(row('ignored', false))).toBe('ignored');
    expect(worldRowKind(row('orphaned'))).toBe('ghost');
    expect(worldRowKind(row('orphaned', false))).toBe('ghost');
    // A Disabled-only name whose file is gone: "Add" would always fail.
    expect(worldRowKind(row('not_added', false))).toBe('ghost');
    expect(worldRowKind(row('not_added', true))).toBe('addable');
    expect(worldRowKind(row('enabled'))).toBe('live');
    expect(worldRowKind(row('disabled', false))).toBe('live');
  });
});
