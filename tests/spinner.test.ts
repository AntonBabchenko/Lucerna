import { render } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import Spinner from '../src/lib/ui/Spinner.svelte';

afterEach(() => {
  vi.useRealTimers();
});

describe('Spinner', () => {
  it('renders immediately when there is no delay, with an accessible status role', () => {
    const { getByRole } = render(Spinner, { props: { label: 'Loading mods' } });
    const status = getByRole('status');
    expect(status.getAttribute('aria-label')).toBe('Loading mods');
    // The visible, spinning element is present and animated.
    expect(status.querySelector('.animate-spin')).not.toBeNull();
    // Screen-reader text mirrors the label.
    expect(status.textContent).toContain('Loading mods');
  });

  it('applies the size class for the chosen size', () => {
    const { getByRole } = render(Spinner, { props: { size: 'lg' } });
    expect(getByRole('status').querySelector('.h-8')).not.toBeNull();
  });

  it('does not render until delayMs has elapsed (anti-flicker)', () => {
    vi.useFakeTimers();
    const { queryByRole } = render(Spinner, { props: { delayMs: 200 } });
    // Nothing shown synchronously — a fast load that resolves before the
    // delay would never flash a spinner.
    expect(queryByRole('status')).toBeNull();

    vi.advanceTimersByTime(199);
    flushSync();
    expect(queryByRole('status')).toBeNull();

    vi.advanceTimersByTime(1);
    flushSync();
    expect(queryByRole('status')).not.toBeNull();
  });
});
