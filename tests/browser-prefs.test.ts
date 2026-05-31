import { beforeEach, describe, expect, test } from 'vitest';

describe('browser-prefs', () => {
  beforeEach(() => localStorage.clear());

  test('defaults to pageSize 20 and grid layout', async () => {
    const { browserPrefs } = await import('../src/lib/mods/browser-prefs.svelte');
    expect(browserPrefs.pageSize).toBe(20);
    expect(browserPrefs.layout).toBe('grid');
  });

  test('falls back to defaults on malformed localStorage', async () => {
    localStorage.setItem('lucerna.browserPrefs', '{not json');
    const m = await import('../src/lib/mods/browser-prefs.svelte');
    expect(m.loadPrefs()).toEqual({ pageSize: 20, layout: 'grid' });
  });

  test('loadPrefs reads a valid stored value', async () => {
    localStorage.setItem('lucerna.browserPrefs', JSON.stringify({ pageSize: 50, layout: 'list' }));
    const m = await import('../src/lib/mods/browser-prefs.svelte');
    expect(m.loadPrefs()).toEqual({ pageSize: 50, layout: 'list' });
  });
});
