import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
import PageSizePicker from '$lib/mods/PageSizePicker.svelte';

describe('PageSizePicker', () => {
  beforeEach(() => {
    browserPrefs.pageSize = 20;
    browserPrefs.installedPageSize = 50;
  });

  it('defaults to the catalog pageSize key', async () => {
    render(PageSizePicker);
    await fireEvent.click(screen.getByTestId('page-size-100'));
    expect(browserPrefs.pageSize).toBe(100);
    expect(browserPrefs.installedPageSize).toBe(50);
  });

  it('writes the installedPageSize key when prefsKey="installedPageSize"', async () => {
    render(PageSizePicker, { props: { prefsKey: 'installedPageSize' } });
    await fireEvent.click(screen.getByTestId('page-size-100'));
    expect(browserPrefs.installedPageSize).toBe(100);
    expect(browserPrefs.pageSize).toBe(20);
  });
});
