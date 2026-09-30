import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// ToastHost now registers a Tauri event listener on mount. Stub the events
// module so tests don't hit the real Tauri IPC (undefined in happy-dom).
vi.mock('$lib/ipc/bindings', () => ({
  events: {
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ToastHost from '$lib/toasts/ToastHost.svelte';
import { dismiss, pushSuccess, pushWarning, toastList } from '$lib/toasts/toasts.svelte';

describe('ToastHost', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    for (const t of [...toastList()]) dismiss(t.id);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders a success toast with its title and a dismiss button', () => {
    pushSuccess('Installed Sodium');
    const { getByText, getByTestId, getByLabelText } = render(ToastHost);
    expect(getByText('Installed Sodium')).toBeTruthy();
    expect(getByTestId('toast-success')).toBeTruthy();
    // A6: success toasts now get a × dismiss button (same as all other levels)
    expect(getByLabelText('Dismiss notification')).toBeTruthy();
  });

  it('renders a warning toast with its title, detail lines and a dismiss button', () => {
    pushWarning('2 mods failed', ['a.jar', 'b.jar']);
    const { getByText, getByLabelText } = render(ToastHost);
    expect(getByText('2 mods failed')).toBeTruthy();
    expect(getByText('a.jar')).toBeTruthy();
    expect(getByText('b.jar')).toBeTruthy();
    expect(getByLabelText('Dismiss notification')).toBeTruthy();
  });

  // Plan §5d L4: a batch report's reason says whom it stopped on a line of its own, above the
  // reason — «Moonlight Lib: Не удалось…» put a capital after a colon.
  it('puts the names a reason stopped on a line of their own, above the reason', () => {
    pushWarning('Fixed 0 of 2', [
      { names: 'Moonlight Lib, ImmediatelyFast', reason: 'Не удалось связаться с сервером.' },
      'In the profile «Alpha»',
    ]);
    const { getByText } = render(ToastHost);
    const names = getByText('Moonlight Lib, ImmediatelyFast');
    const reason = getByText('Не удалось связаться с сервером.');
    const item = names.parentElement as HTMLElement;
    expect(item.tagName).toBe('LI');
    expect(reason.parentElement).toBe(item);
    expect(names.compareDocumentPosition(reason) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(names.classList).toContain('block');
    expect(reason.classList).toContain('block');
    // No colon between them, and read as two phrases, not one run-on.
    expect(item.textContent?.trim()).toBe(
      'Moonlight Lib, ImmediatelyFast Не удалось связаться с сервером.',
    );
    expect(getByText('In the profile «Alpha»').tagName).toBe('LI');
  });

  it('clicking the dismiss button removes the warning toast', async () => {
    pushWarning('failed');
    const { getByLabelText, queryByText } = render(ToastHost);
    expect(queryByText('failed')).toBeTruthy();
    await fireEvent.click(getByLabelText('Dismiss notification'));
    expect(queryByText('failed')).toBeNull();
  });

  // Regression guard: long detail lines must WRAP, not get clipped to a single
  // ellipsised row. `truncate` (white-space:nowrap + overflow:hidden + ellipsis)
  // was the bug — a long line like the MS pending-approval message was cut off.
  it('detail lines wrap (break-words) instead of being truncated', () => {
    pushWarning('Title', ['Microsoft has not yet approved the app registration.']);
    const { container } = render(ToastHost);
    const line = container.querySelector('li');
    expect(line).toBeTruthy();
    expect(line?.className).toContain('break-words');
    expect(line?.className).not.toContain('truncate');
  });
});
