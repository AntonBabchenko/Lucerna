import { beforeEach, describe, expect, it, test } from 'vitest';
import { loadPrefs } from '$lib/mods/browser-prefs.svelte';

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
    expect(m.loadPrefs()).toEqual({
      pageSize: 20,
      layout: 'grid',
      installedPageSize: 50,
      density: 'comfortable',
    });
  });

  test('loadPrefs reads a valid stored value', async () => {
    localStorage.setItem('lucerna.browserPrefs', JSON.stringify({ pageSize: 50, layout: 'list' }));
    const m = await import('../src/lib/mods/browser-prefs.svelte');
    expect(m.loadPrefs()).toEqual({
      pageSize: 50,
      layout: 'list',
      installedPageSize: 50,
      density: 'comfortable',
    });
  });
});

describe('browser-prefs installedPageSize', () => {
  beforeEach(() => localStorage.clear());

  it('defaults installedPageSize to 50', () => {
    expect(loadPrefs().installedPageSize).toBe(50);
  });

  it('reads a persisted installedPageSize', () => {
    localStorage.setItem(
      'lucerna.browserPrefs',
      JSON.stringify({ pageSize: 20, layout: 'grid', installedPageSize: 100 }),
    );
    expect(loadPrefs().installedPageSize).toBe(100);
  });

  it('falls back to 50 for an invalid persisted installedPageSize', () => {
    localStorage.setItem('lucerna.browserPrefs', JSON.stringify({ installedPageSize: 999 }));
    expect(loadPrefs().installedPageSize).toBe(50);
  });
});

describe('browser-prefs density', () => {
  beforeEach(() => localStorage.clear());

  it('defaults density to comfortable', () => {
    expect(loadPrefs().density).toBe('comfortable');
  });

  it('reads a persisted density', () => {
    localStorage.setItem(
      'lucerna.browserPrefs',
      JSON.stringify({ pageSize: 20, layout: 'grid', installedPageSize: 50, density: 'compact' }),
    );
    expect(loadPrefs().density).toBe('compact');
  });

  it('falls back to comfortable for an invalid persisted density', () => {
    localStorage.setItem('lucerna.browserPrefs', JSON.stringify({ density: 'nope' }));
    expect(loadPrefs().density).toBe('comfortable');
  });
});
