import { describe, expect, it } from 'vitest';
import { datapackWorldSummary } from '$lib/worlds/datapacks-gating';

type P = Parameters<typeof datapackWorldSummary>[0][number];
const p = (
  state: P['state'],
  level_dat: P['level_dat'] = 'present',
  ignored_reason: P['ignored_reason'] = null,
): P => ({ state, level_dat, ignored_reason });

describe('datapackWorldSummary', () => {
  it('zero placements is the accent "in no world" state, not an absent value', () => {
    const s = datapackWorldSummary([]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryInNoWorld');
    expect(s.checked?.emphasis).toBe('accent');
    expect(s.unchecked).toBe(0);
  });

  it('all-disabled reads "disabled everywhere"', () => {
    const s = datapackWorldSummary([p('disabled'), p('disabled')]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryDisabledEverywhere');
    expect(s.checked?.args.total).toBe(2);
    expect(s.checked?.emphasis).toBe('muted');
  });

  it('a mix counts enabled out of total', () => {
    const s = datapackWorldSummary([p('enabled'), p('disabled'), p('enabled')]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.checked?.args).toEqual({ enabled: 2, total: 3 });
    expect(s.unchecked).toBe(0);
  });

  // A world Lucerna could not check may or may not have the pack switched
  // on. Counting it in "of N" read as "not enabled there": a pack whose only
  // world could not be checked read "Enabled in 0 of 1 world". It is shown
  // apart instead, and counted in neither number.
  it('an unknown state is shown apart, never counted as not enabled', () => {
    const s = datapackWorldSummary([p('enabled'), p(null)]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.checked?.args).toEqual({ enabled: 1, total: 1 });
    expect(s.unchecked).toBe(1);
  });

  it('a pack whose only world could not be checked claims nothing about it', () => {
    const s = datapackWorldSummary([p(null)]);
    // Neither "Enabled in 0 of 1 world" nor "In no world": both are guesses.
    expect(s.checked).toBeNull();
    expect(s.unchecked).toBe(1);
  });

  it('an unknown state blocks the "disabled everywhere" claim', () => {
    // We could not read that world — asserting "everywhere off" about it
    // would be a guess presented as fact.
    const s = datapackWorldSummary([p('disabled'), p(null)]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.checked?.args).toEqual({ enabled: 0, total: 1 });
    expect(s.unchecked).toBe(1);
  });

  it('an orphaned placement is neither enabled nor "disabled everywhere"', () => {
    const s = datapackWorldSummary([p('orphaned')]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.checked?.args).toEqual({ enabled: 0, total: 1 });
  });

  it('an ignored placement is left out of the total (§0.5 A22)', () => {
    expect(datapackWorldSummary([p('enabled'), p('ignored')]).checked?.args).toEqual({
      enabled: 1,
      total: 1,
    });
  });

  // Departs from §0.5 A22 on purpose (honesty over its letter): the game
  // restores an only-old world from level.dat_old and loads the packs that
  // file holds, so the row counts it by that state. Only a folder with no
  // level file, which the game never opens, is left out.
  it('a folder with no level file is left out; an only-old world counts by its state', () => {
    const s = datapackWorldSummary([p('disabled'), p(null, 'absent'), p('disabled', 'only_old')]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryDisabledEverywhere');
    expect(s.checked?.args.total).toBe(2);
    expect(s.unchecked).toBe(0);
  });

  it('a pack enabled only in an only-old world is not "in no world"', () => {
    const s = datapackWorldSummary([p('enabled', 'only_old')]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.checked?.args).toEqual({ enabled: 1, total: 1 });
  });

  it('a pack whose every placement is ignored is in no world', () => {
    const s = datapackWorldSummary([p('ignored')]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryInNoWorld');
    expect(s.unchecked).toBe(0);
  });

  // Fallback Q2: "Couldn't check" is not "the game ignores it". Like an unknown
  // state, it is shown apart and blocks both the "disabled everywhere" and the
  // "in no world" claims.
  it('an entry Lucerna could not read counts as unknown, not as ignored', () => {
    const alone = datapackWorldSummary([p('ignored', 'present', 'unreadable')]);
    expect(alone.checked).toBeNull();
    expect(alone.unchecked).toBe(1);
    const withDisabled = datapackWorldSummary([
      p('disabled'),
      p('ignored', 'present', 'unreadable'),
    ]);
    expect(withDisabled.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(withDisabled.checked?.args).toEqual({ enabled: 0, total: 1 });
    expect(withDisabled.unchecked).toBe(1);
  });

  it('leaves ignored placements and folders with no level file out of both counts', () => {
    const s = datapackWorldSummary([
      p('enabled'),
      p('ignored'),
      p(null, 'absent'),
      p('disabled', 'only_old'),
      p(null, null),
    ]);
    expect(s.checked?.key).toBe('addons.datapacks.summaryEnabledIn');
    // A null level_dat is "could not tell": apart, like a null state.
    expect(s.checked?.args).toEqual({ enabled: 1, total: 2 });
    expect(s.unchecked).toBe(1);
  });
});
