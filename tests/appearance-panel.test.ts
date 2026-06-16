// tests/appearance-panel.test.ts
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { rainbowFx } from '../src/lib/fx/rainbow-fx.svelte';
import AppearancePanel from '../src/lib/settings/AppearancePanel.svelte';

describe('AppearancePanel', () => {
  it('renders the three theme radios with their data-testids', () => {
    const { container } = render(AppearancePanel);
    for (const v of ['system', 'light', 'dark']) {
      const input = container.querySelector(`[data-testid="theme-${v}"]`);
      expect(input).not.toBeNull();
      expect(input?.getAttribute('type')).toBe('radio');
    }
  });

  it('renders the language selector', () => {
    render(AppearancePanel);
    expect(screen.getByTestId('language-select')).toBeTruthy();
  });

  it('rainbow toggle reflects the rainbow preference', () => {
    rainbowFx.set(true);
    render(AppearancePanel);
    const toggle = screen.getByTestId('rainbow-icons-toggle') as HTMLInputElement;
    expect(toggle.checked).toBe(true);
  });
});
