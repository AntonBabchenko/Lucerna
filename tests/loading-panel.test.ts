import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import LoadingPanel from '../src/lib/ui/LoadingPanel.svelte';

describe('LoadingPanel', () => {
  it('renders a centered spinner with the visible label', () => {
    render(LoadingPanel, { props: { label: 'Loading installed mods', delayMs: 0 } });
    const status = screen.getByRole('status');
    expect(status.getAttribute('aria-label')).toBe('Loading installed mods');
    expect(status.className).toContain('flex-col');
    expect(status.querySelector('[aria-hidden="true"].text-sm')?.textContent).toBe(
      'Loading installed mods',
    );
  });
});
