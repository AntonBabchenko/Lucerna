import { describe, expect, it } from 'vitest';
import { buildAttentionItems } from '$lib/overview/attention';

const none = {
  mcVersionMissing: false,
  missingModsCount: 0,
  incompatibleCount: 0,
  integrityProblemCount: 0,
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
    });
    expect(items.map((i) => i.kind)).toEqual([
      'pick_version',
      'missing_mods',
      'incompatible',
      'integrity',
    ]);
  });

  it('carries the count on counted items and 0 on pick_version', () => {
    const items = buildAttentionItems({ ...none, mcVersionMissing: true, missingModsCount: 5 });
    expect(items).toEqual([
      { kind: 'pick_version', count: 0 },
      { kind: 'missing_mods', count: 5 },
    ]);
  });

  it('omits signals whose count is zero', () => {
    expect(buildAttentionItems({ ...none, incompatibleCount: 0, integrityProblemCount: 4 })).toEqual(
      [{ kind: 'integrity', count: 4 }],
    );
  });
});
