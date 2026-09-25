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

  it('renders no detail line unless one is asked for', () => {
    // (pin)
    render(LoadingPanel, { props: { label: 'Loading', delayMs: 0 } });
    expect(screen.queryByTestId('loading-panel-detail')).toBeNull();
  });

  it('keeps the detail line a polite live region that exists before its first text', async () => {
    const { rerender } = render(LoadingPanel, {
      props: { label: 'Loading', delayMs: 0, detail: null },
    });
    // Present and empty: a region created WITH its text is often not announced.
    const line = screen.getByTestId('loading-panel-detail');
    expect(line.getAttribute('aria-live')).toBe('polite');
    expect(line.textContent?.trim()).toBe('');
    await rerender({ label: 'Loading', delayMs: 0, detail: '3 of 9' });
    expect(screen.getByTestId('loading-panel-detail').textContent?.trim()).toBe('3 of 9');
  });
});
