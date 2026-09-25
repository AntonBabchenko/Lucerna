import { describe, expect, it } from 'vitest';
import type { WorldPackState } from '$lib/ipc/bindings';
import {
  datapacksDisabledKey,
  levelDatBlockedKey,
  worldDatapacksDisabledKey,
  worldRowKind,
} from '$lib/worlds/datapacks-gating';

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

describe('levelDatBlockedKey', () => {
  it('names absent and only-old, and nothing else', () => {
    expect(levelDatBlockedKey('absent')).toBe('worlds.datapacks.blockedNoLevelDat');
    expect(levelDatBlockedKey('only_old')).toBe('worlds.datapacks.blockedOnlyOld');
    expect(levelDatBlockedKey('present')).toBeNull();
    // Could not tell is not a verdict: the backend re-checks before any write.
    expect(levelDatBlockedKey(null)).toBeNull();
  });
});

describe('worldDatapacksDisabledKey', () => {
  it('ranks running > level.dat > busy', () => {
    expect(worldDatapacksDisabledKey({ running: true, busy: true, levelDat: 'only_old' })).toBe(
      'worlds.datapacks.blockedRunning',
    );
    expect(worldDatapacksDisabledKey({ running: false, busy: true, levelDat: 'only_old' })).toBe(
      'worlds.datapacks.blockedOnlyOld',
    );
    expect(worldDatapacksDisabledKey({ running: false, busy: true, levelDat: 'absent' })).toBe(
      'worlds.datapacks.blockedNoLevelDat',
    );
    expect(worldDatapacksDisabledKey({ running: false, busy: true, levelDat: 'present' })).toBe(
      'worlds.datapacks.blockedBusy',
    );
    expect(worldDatapacksDisabledKey({ running: false, busy: false, levelDat: null })).toBeNull();
  });
});
