// Toasts group intent coverage — extends tests/toast-opacity.test.ts (cluster D)
// which already guards bg-success-bg / bg-warning-bg positive assertions.
// This file adds: text colour, border colour, title font-weight, list-item
// layout, and close-button variant for both toast kinds.

import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// ToastHost registers a Tauri event listener on mount. Stub the events
// module so tests don't hit the real Tauri IPC (undefined in happy-dom).
vi.mock('$lib/ipc/bindings', () => ({
  events: {
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ToastHost from '$lib/toasts/ToastHost.svelte';
import { dismiss, pushSuccess, pushWarning, toastList } from '$lib/toasts/toasts.svelte';

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

describe('Success toast — full intent coverage', () => {
  it('has border-success class', () => {
    pushSuccess('Test');
    const { container } = render(ToastHost);
    const t = getToast(container, 'success');
    expect(t.className).toContain('border-success');
  });

  it('has text-success class', () => {
    pushSuccess('Test');
    const { container } = render(ToastHost);
    const t = getToast(container, 'success');
    expect(t.className).toContain('text-success');
  });

  it('title is rendered in a font-medium span', () => {
    pushSuccess('Test title');
    const { container } = render(ToastHost);
    const t = getToast(container, 'success');
    const title = t.querySelector('.font-medium') as HTMLElement;
    expect(title).not.toBeNull();
    expect(title.textContent).toBe('Test title');
  });

  it('close button carries .btn-icon variant', () => {
    pushSuccess('Test');
    const { container } = render(ToastHost);
    const t = getToast(container, 'success');
    const btn = t.querySelector('button[aria-label="Dismiss notification"]') as HTMLElement;
    expect(btn).not.toBeNull();
    expect(btn).toHaveBtnVariant('icon');
  });

  it('no list items rendered when lines is empty', () => {
    pushSuccess('Title');
    const { container } = render(ToastHost);
    const t = getToast(container, 'success');
    const items = t.querySelectorAll('li');
    expect(items.length).toBe(0);
  });
});

describe('Warning toast — full intent coverage', () => {
  it('has border-warning-text class', () => {
    pushWarning('Heads up');
    const { container } = render(ToastHost);
    const t = getToast(container, 'warning');
    expect(t.className).toContain('border-warning-text');
  });

  it('has text-warning-text class', () => {
    pushWarning('Heads up');
    const { container } = render(ToastHost);
    const t = getToast(container, 'warning');
    expect(t.className).toContain('text-warning-text');
  });

  it('title is rendered in a font-medium span', () => {
    pushWarning('Alert title');
    const { container } = render(ToastHost);
    const t = getToast(container, 'warning');
    const title = t.querySelector('.font-medium') as HTMLElement;
    expect(title).not.toBeNull();
    expect(title.textContent).toBe('Alert title');
  });

  it('close button carries .btn-icon variant', () => {
    pushWarning('Alert');
    const { container } = render(ToastHost);
    const t = getToast(container, 'warning');
    const btn = t.querySelector('button[aria-label="Dismiss notification"]') as HTMLElement;
    expect(btn).not.toBeNull();
    expect(btn).toHaveBtnVariant('icon');
  });

  it('list items wrap (break-words) instead of truncating to one line', () => {
    // Long detail lines (e.g. the MS pending-approval message) must wrap, not
    // get clipped to a single ellipsised row — so `break-words`, not `truncate`.
    pushWarning('Title', ['line1', 'line2', 'line3']);
    const { container } = render(ToastHost);
    const t = getToast(container, 'warning');
    const items = t.querySelectorAll('li');
    expect(items.length).toBe(3);
    for (const li of items) {
      expect(li.className).toContain('break-words');
      expect(li.className).not.toContain('truncate');
    }
  });

  it('list items are inside a text-xs ul', () => {
    pushWarning('Title', ['alpha', 'beta']);
    const { container } = render(ToastHost);
    const t = getToast(container, 'warning');
    const ul = t.querySelector('ul') as HTMLElement;
    expect(ul).not.toBeNull();
    expect(ul.className).toContain('text-xs');
  });
});
