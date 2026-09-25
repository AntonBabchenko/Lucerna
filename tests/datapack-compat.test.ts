// compatLine: the one sentence per verdict, shared by the world tab and the
// library row (§1 C5), so they can never word the same verdict differently.
import { describe, expect, it } from 'vitest';
import { compatLine, wontLoadKey } from '$lib/worlds/datapack-compat';
import { ignoredHintKey } from '$lib/worlds/datapack-state';

describe('compatLine', () => {
  it('too_old and too_new carry both labels', () => {
    expect(compatLine({ kind: 'too_old', made_for: '34–48', game: '94.1' })).toEqual({
      key: 'worlds.datapacks.compatTooOld',
      args: { madeFor: '34–48', game: '94.1' },
    });
    expect(compatLine({ kind: 'too_new', made_for: '107.1', game: '94.1' })).toEqual({
      key: 'worlds.datapacks.compatTooNew',
      args: { madeFor: '107.1', game: '94.1' },
    });
  });
  it('broken has no values', () => {
    expect(compatLine({ kind: 'broken' })).toEqual({
      key: 'worlds.datapacks.compatBroken',
      args: {},
    });
  });
  it.each([
    ['no_pack_mcmeta', 'worlds.datapacks.compatWontLoadNoPackMcmeta'],
    ['no_pack_section', 'worlds.datapacks.compatWontLoadNoPackSection'],
    ['no_description', 'worlds.datapacks.compatWontLoadNoDescription'],
    ['no_pack_format', 'worlds.datapacks.compatWontLoadNoPackFormat'],
  ] as const)('wont_load %s names its own reason', (reason, key) => {
    expect(wontLoadKey(reason)).toBe(key);
    expect(compatLine({ kind: 'wont_load', reason })).toEqual({ key, args: {} });
  });
  it('compatible and unknown say nothing (unknown keeps its own neutral badge)', () => {
    expect(compatLine({ kind: 'compatible' })).toBeNull();
    expect(compatLine({ kind: 'unknown' })).toBeNull();
  });
});

describe('ignoredHintKey — not_loadable', () => {
  it('names the row’s own wont_load reason (§0.5 A12)', () => {
    expect(ignoredHintKey('not_loadable', { kind: 'wont_load', reason: 'no_description' })).toBe(
      'worlds.datapacks.compatWontLoadNoDescription',
    );
  });
  it('says only "unknown" without a wont_load verdict to name', () => {
    expect(ignoredHintKey('not_loadable')).toBe('worlds.datapacks.compatUnknown');
    expect(ignoredHintKey('not_loadable', { kind: 'unknown' })).toBe(
      'worlds.datapacks.compatUnknown',
    );
  });
});
