import { describe, expect, it } from 'vitest';
import { activeCount } from '$lib/browse/filter-model';

describe('filter-model activeCount', () => {
  it('is 0 when nothing is narrowed', () => {
    expect(activeCount({ loader: '', mc: '' })).toBe(0);
  });

  it('counts the loader facet', () => {
    expect(activeCount({ loader: 'neoforge', mc: '' })).toBe(1);
  });

  it('counts the mc facet', () => {
    expect(activeCount({ loader: '', mc: '1.21.1' })).toBe(1);
  });

  it('counts showInstalled only when explicitly false (hide active)', () => {
    expect(activeCount({ loader: '', mc: '', showInstalled: true })).toBe(0);
    expect(activeCount({ loader: '', mc: '', showInstalled: false })).toBe(1);
  });

  it('counts all active narrowing facets together', () => {
    expect(activeCount({ loader: 'fabric', mc: '1.21.1', showInstalled: false })).toBe(3);
  });
});
