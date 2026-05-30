// Regression guard: toasts must use opaque colored backgrounds (bg-success-bg
// for success, bg-warning-bg for warning), NOT translucent tints like
// `bg-success/10` or `bg-warning-bg/10` that let the background content bleed
// through the notification chrome. The translucent form was the original B2a
// regression (commit ebd27f6 first swapped to bg-surface; later refined to
// opaque colored bg for visual identity).

import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ToastHost from '$lib/toasts/ToastHost.svelte';
import { dismiss, pushSuccess, pushWarning, toastList } from '$lib/toasts/toasts.svelte';

// The toast store is module-global state shared across tests. Clear it in
// beforeEach. Fake timers prevent the success auto-dismiss timer (SUCCESS_TTL_MS)
// from firing mid-test and removing the toast before we can inspect its classes.
beforeEach(() => {
  vi.useFakeTimers();
  for (const t of [...toastList()]) dismiss(t.id);
});
afterEach(() => {
  vi.useRealTimers();
});

function getToast(container: HTMLElement, kind: string): HTMLElement {
  const el = container.querySelector(`[data-testid="toast-${kind}"]`);
  if (!el) throw new Error(`Toast of kind "${kind}" not found in DOM`);
  return el as HTMLElement;
}

describe('ToastHost — coloured-bg + opacity regression guard', () => {
  it('success toast uses opaque bg-success-bg (coloured, not neutral, not translucent)', () => {
    pushSuccess('Test title');
    const { container } = render(ToastHost);
    const toast = getToast(container, 'success');
    expect(toast.className).toContain('bg-success-bg');
    expect(toast.className).not.toContain('bg-surface');
    expect(toast.className).not.toMatch(/bg-success(?:-bg)?\/\d{1,2}\b/);
  });

  it('warning toast uses opaque bg-warning-bg (coloured, not neutral, not translucent)', () => {
    pushWarning('Test title');
    const { container } = render(ToastHost);
    const toast = getToast(container, 'warning');
    expect(toast.className).toContain('bg-warning-bg');
    expect(toast.className).not.toContain('bg-surface');
    expect(toast.className).not.toMatch(/bg-warning-bg\/\d{1,2}\b/);
  });
});
