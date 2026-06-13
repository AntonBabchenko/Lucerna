import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import DensityToggle from '$lib/mods/DensityToggle.svelte';
import { browserPrefs } from '$lib/mods/browser-prefs.svelte';

describe('DensityToggle', () => {
  beforeEach(() => {
    browserPrefs.density = 'comfortable';
  });

  it('reflects the current density via aria-pressed', () => {
    render(DensityToggle);
    const comfortable = screen.getByRole('button', { name: /comfortable/i });
    expect(comfortable.getAttribute('aria-pressed')).toBe('true');
  });

  it('switches density when the compact button is clicked', async () => {
    render(DensityToggle);
    await fireEvent.click(screen.getByRole('button', { name: /compact/i }));
    expect(browserPrefs.density).toBe('compact');
  });
});
