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
    });
    expect(items.map((i) => i.kind)).toEqual([
      'pick_version',
      'missing_mods',
      'incompatible',
      'integrity',
      'modpack_update',
    ]);
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

  it('omits signals whose count is zero', () => {
    expect(
      buildAttentionItems({ ...none, incompatibleCount: 0, integrityProblemCount: 4 }),
    ).toEqual([{ kind: 'integrity', count: 4 }]);
  });
});
