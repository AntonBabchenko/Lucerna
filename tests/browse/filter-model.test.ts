import { describe, expect, it } from 'vitest';
import { activeChips, activeCount } from '$lib/browse/filter-model';

describe('filter-model', () => {
  it('returns no chips when nothing is narrowed', () => {
    expect(activeChips({ loader: '', mc: '' })).toEqual([]);
    expect(activeCount({ loader: '', mc: '' })).toBe(0);
  });

  it('emits a loader chip with the display label', () => {
    const chips = activeChips({ loader: 'neoforge', mc: '' });
    expect(chips).toEqual([{ key: 'loader', label: 'NeoForge' }]);
  });

  it('emits an mc chip using the raw version string', () => {
    expect(activeChips({ loader: '', mc: '1.21.1' })).toEqual([{ key: 'mc', label: '1.21.1' }]);
  });

  it('emits a showInstalled chip only when explicitly false', () => {
    expect(activeChips({ loader: '', mc: '', showInstalled: true })).toEqual([]);
    expect(activeChips({ loader: '', mc: '', showInstalled: false })).toEqual([
      { key: 'showInstalled', label: 'Installed hidden' },
    ]);
  });

  it('counts all active narrowing facets together', () => {
    expect(activeCount({ loader: 'fabric', mc: '1.21.1', showInstalled: false })).toBe(3);
  });
});
