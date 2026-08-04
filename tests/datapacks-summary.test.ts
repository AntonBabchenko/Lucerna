import { describe, expect, it } from 'vitest';
import { datapackWorldSummary } from '$lib/worlds/datapacks-gating';

type P = Parameters<typeof datapackWorldSummary>[0][number];
const p = (state: P['state']): P => ({ state });

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
});
