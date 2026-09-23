// tests/appearance-panel.test.ts
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { rainbowFx } from '../src/lib/fx/rainbow-fx.svelte';
import AppearancePanel from '../src/lib/settings/AppearancePanel.svelte';

describe('AppearancePanel', () => {
  it('renders the three theme options as aria-pressed buttons inside one group named "Theme"', () => {
    render(AppearancePanel);
    const groups = screen.getAllByRole('group', { name: 'Theme' });
    expect(groups).toHaveLength(1);
    for (const v of ['system', 'light', 'dark']) {
      const btn = screen.getByTestId(`theme-${v}`);
      expect(btn.tagName).toBe('BUTTON');
      expect(btn.getAttribute('aria-pressed')).not.toBeNull();
      expect(groups[0].contains(btn)).toBe(true);
    }
  });

  it('describes the theme group with the hint under it', () => {
    render(AppearancePanel);
    const group = screen.getByRole('group', { name: 'Theme' });
    const id = group.getAttribute('aria-describedby');
    expect(id).toBeTruthy();
    expect(document.getElementById(id as string)?.textContent).toContain('operating system');
  });

  it('opens the theme block with a heading, so the search label word is on the page', () => {
    render(AppearancePanel);
    expect(screen.getByRole('heading', { name: 'Theme', level: 3 })).toBeTruthy();
  });

  it('groups the two icon effects under an "Effects" heading', () => {
    render(AppearancePanel);
    expect(screen.getByRole('heading', { name: 'Effects', level: 3 })).toBeTruthy();
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
