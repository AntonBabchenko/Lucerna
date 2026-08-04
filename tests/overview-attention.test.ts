import { describe, expect, it } from 'vitest';
import { buildAttentionItems } from '$lib/overview/attention';

const none = {
  mcVersionMissing: false,
  missingModsCount: 0,
  incompatibleCount: 0,
  integrityProblemCount: 0,
  hasModpackUpdate: false,
  hasLogIssue: false,
  logFixAvailable: false,
  serverFixAvailable: false,
  preflightUnknown: false,
};

describe('buildAttentionItems', () => {
  it('returns an empty list when nothing needs attention', () => {
    expect(buildAttentionItems(none)).toEqual([]);
  });

  it('emits one item per active signal in a fixed order', () => {
    const items = buildAttentionItems({
      mcVersionMissing: true,
      missingModsCount: 3,
      incompatibleCount: 1,
      integrityProblemCount: 2,
      hasModpackUpdate: true,
      hasLogIssue: false,
      logFixAvailable: false,
      serverFixAvailable: false,
      preflightUnknown: false,
    });
    expect(items.map((i) => i.kind)).toEqual([
      'pick_version',
      'missing_mods',
      'incompatible',
      'integrity',
      'modpack_update',
    ]);
  });

  it('appends a server_log_fix item (count 0) last when a server fix is available', () => {
    const items = buildAttentionItems({ ...none, serverFixAvailable: true });
    expect(items).toEqual([{ kind: 'server_log_fix', count: 0 }]);
  });

  it('appends a modpack_update item (count 0) last when an update is available', () => {
    const items = buildAttentionItems({ ...none, hasModpackUpdate: true });
    expect(items).toEqual([{ kind: 'modpack_update', count: 0 }]);
  });

  it('omits modpack_update when no update is available', () => {
    const items = buildAttentionItems({ ...none, integrityProblemCount: 4 });
    expect(items.some((i) => i.kind === 'modpack_update')).toBe(false);
  });

  it('carries the count on counted items and 0 on pick_version', () => {
    const items = buildAttentionItems({ ...none, mcVersionMissing: true, missingModsCount: 5 });
    expect(items).toEqual([
      { kind: 'pick_version', count: 0 },
      { kind: 'missing_mods', count: 5 },
    ]);
  });

  it('emits preflight_unknown (count 0) when the dependency check could not run', () => {
    expect(buildAttentionItems({ ...none, preflightUnknown: true })).toEqual([
      { kind: 'preflight_unknown', count: 0 },
    ]);
  });

  it('orders "could not check" after the confirmed problems and before the server signal', () => {
    // "We could not check" is weaker news than "here is what is wrong", so it
    // must never push a real problem down the list; the global server signal
    // stays last because it is not about this instance at all.
    const items = buildAttentionItems({
      ...none,
      incompatibleCount: 2,
      preflightUnknown: true,
      serverFixAvailable: true,
    });
    expect(items.map((i) => i.kind)).toEqual([
      'incompatible',
      'preflight_unknown',
      'server_log_fix',
    ]);
  });

  it('omits signals whose count is zero', () => {
    expect(
      buildAttentionItems({ ...none, incompatibleCount: 0, integrityProblemCount: 4 }),
    ).toEqual([{ kind: 'integrity', count: 4 }]);
  });
});
