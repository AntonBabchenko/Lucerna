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
    expect(s.key).toBe('addons.datapacks.summaryInNoWorld');
    expect(s.emphasis).toBe('accent');
  });

  it('all-disabled reads "disabled everywhere"', () => {
    const s = datapackWorldSummary([p('disabled'), p('disabled')]);
    expect(s.key).toBe('addons.datapacks.summaryDisabledEverywhere');
    expect(s.args.total).toBe(2);
    expect(s.emphasis).toBe('muted');
  });

  it('a mix counts enabled out of total', () => {
    const s = datapackWorldSummary([p('enabled'), p('disabled'), p('enabled')]);
    expect(s.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.args).toEqual({ enabled: 2, total: 3 });
  });

  it('an unknown state counts toward the total but never toward enabled', () => {
    const s = datapackWorldSummary([p('enabled'), p(null)]);
    expect(s.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.args).toEqual({ enabled: 1, total: 2 });
  });

  it('an unknown state blocks the "disabled everywhere" claim', () => {
    // We could not read that world's level.dat — asserting "everywhere off"
    // about it would be a guess presented as fact.
    const s = datapackWorldSummary([p('disabled'), p(null)]);
    expect(s.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.args).toEqual({ enabled: 0, total: 2 });
  });

  it('an orphaned placement is neither enabled nor "disabled everywhere"', () => {
    const s = datapackWorldSummary([p('orphaned')]);
    expect(s.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(s.args).toEqual({ enabled: 0, total: 1 });
  });

  it('an ignored placement is left out of the total (§0.5 A22)', () => {
    expect(datapackWorldSummary([p('enabled'), p('ignored')]).args).toEqual({
      enabled: 1,
      total: 1,
    });
  });

  it('a folder without a usable level.dat is left out of the total', () => {
    const s = datapackWorldSummary([p('disabled'), p(null, 'absent'), p('disabled', 'only_old')]);
    expect(s.key).toBe('addons.datapacks.summaryDisabledEverywhere');
    expect(s.args.total).toBe(1);
  });

  it('a pack whose every placement is ignored is in no world', () => {
    expect(datapackWorldSummary([p('ignored')]).key).toBe('addons.datapacks.summaryInNoWorld');
  });

  // Fallback Q2: "Couldn't check" is not "the game ignores it". Like an unknown
  // state, it counts toward the total and blocks both the "disabled
  // everywhere" and the "in no world" claims.
  it('an entry Lucerna could not read counts as unknown, not as ignored', () => {
    const alone = datapackWorldSummary([p('ignored', 'present', 'unreadable')]);
    expect(alone.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(alone.args).toEqual({ enabled: 0, total: 1 });
    const withDisabled = datapackWorldSummary([
      p('disabled'),
      p('ignored', 'present', 'unreadable'),
    ]);
    expect(withDisabled.key).toBe('addons.datapacks.summaryEnabledIn');
    expect(withDisabled.args).toEqual({ enabled: 0, total: 2 });
  });

  it('leaves ignored and non-present placements out of the total', () => {
    const s = datapackWorldSummary([
      p('enabled'),
      p('ignored'),
      p(null, 'absent'),
      p('disabled', 'only_old'),
      p(null, null),
    ]);
    expect(s.key).toBe('addons.datapacks.summaryEnabledIn');
    // A null level_dat is "could not tell" and still counts, like a null state.
    expect(s.args).toEqual({ enabled: 1, total: 2 });
  });
});
