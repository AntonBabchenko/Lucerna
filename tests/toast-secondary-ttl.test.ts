// A toast can carry a second, quieter action, and a timed toast waits while
// someone is reading it: its countdown pauses on hover and focus and resumes
// with the time that was left. The × only ever closes.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  events: { gpuPrefApplied: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));

import ToastHost from '$lib/toasts/ToastHost.svelte';
import { dismiss, pushActionToast, toastList } from '$lib/toasts/toasts.svelte';

beforeEach(() => {
  for (const t of [...toastList()]) dismiss(t.id);
});
afterEach(() => {
  vi.useRealTimers();
});

describe('a second action on a toast', () => {
  it('renders after the first, quieter, and runs then closes', async () => {
    const skip = vi.fn();
    render(ToastHost);
    pushActionToast(
      'info',
      'Version 9.9.9 is available',
      { label: 'Update now', run: () => {} },
      [],
      {
        secondary: { label: 'Skip this version', run: skip },
      },
    );
    const second = await screen.findByRole('button', { name: 'Skip this version' });
    const first = screen.getByRole('button', { name: 'Update now' });
    expect(second.className).toContain('btn-ghost');
    expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    await fireEvent.click(second);
    expect(skip).toHaveBeenCalledTimes(1);
    expect(toastList()).toHaveLength(0);
  });

  it('is not what the × does: closing runs neither action', async () => {
    const run = vi.fn();
    const skip = vi.fn();
    render(ToastHost);
    pushActionToast('info', 'X', { label: 'Go', run }, [], {
      secondary: { label: 'Skip', run: skip },
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Dismiss notification' }));
    expect(run).not.toHaveBeenCalled();
    expect(skip).not.toHaveBeenCalled();
    expect(toastList()).toHaveLength(0);
  });
});

describe('a timed toast', () => {
  it('hides itself after its time', () => {
    vi.useFakeTimers();
    pushActionToast('info', 'T', { label: 'Go', run: () => {} }, [], { ttlMs: 5000 });
    vi.advanceTimersByTime(4999);
    expect(toastList()).toHaveLength(1);
    vi.advanceTimersByTime(2);
    expect(toastList()).toHaveLength(0);
  });

  it('waits while hovered, then finishes the time that was left', async () => {
    vi.useFakeTimers();
    render(ToastHost);
    pushActionToast('info', 'Hover me', { label: 'Go', run: () => {} }, [], { ttlMs: 5000 });
    await vi.advanceTimersByTimeAsync(4000);
    const card = screen.getByTestId('toast-info');
    await fireEvent.pointerEnter(card);
    await vi.advanceTimersByTimeAsync(20000);
    expect(toastList()).toHaveLength(1);
    await fireEvent.pointerLeave(card);
    await vi.advanceTimersByTimeAsync(990);
    expect(toastList()).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(20);
    expect(toastList()).toHaveLength(0);
  });

  it('waits while it holds keyboard focus', async () => {
    vi.useFakeTimers();
    render(ToastHost);
    pushActionToast('info', 'Focus me', { label: 'Go', run: () => {} }, [], { ttlMs: 5000 });
    await vi.advanceTimersByTimeAsync(1000);
    const card = screen.getByTestId('toast-info');
    await fireEvent.focusIn(screen.getByRole('button', { name: 'Go' }));
    await vi.advanceTimersByTimeAsync(20000);
    expect(toastList()).toHaveLength(1);
    await fireEvent.focusOut(card, { relatedTarget: document.body });
    await vi.advanceTimersByTimeAsync(4010);
    expect(toastList()).toHaveLength(0);
  });

  it('resumes after keyboard focus walked through several of its buttons and left', async () => {
    // focusin bubbles on every hop between the toast's own buttons; only the
    // final exit is a leave. Counting each hop as a new pause left the toast
    // stuck on screen for good.
    vi.useFakeTimers();
    render(ToastHost);
    pushActionToast('info', 'Tab through me', { label: 'Go', run: () => {} }, [], {
      ttlMs: 5000,
      secondary: { label: 'Skip', run: () => {} },
    });
    await vi.advanceTimersByTimeAsync(1000);
    const close = screen.getByRole('button', { name: 'Dismiss notification' });
    const go = screen.getByRole('button', { name: 'Go' });
    const skip = screen.getByRole('button', { name: 'Skip' });
    await fireEvent.focusIn(close);
    await fireEvent.focusOut(close, { relatedTarget: go });
    await fireEvent.focusIn(go);
    await fireEvent.focusOut(go, { relatedTarget: skip });
    await fireEvent.focusIn(skip);
    await vi.advanceTimersByTimeAsync(20000);
    expect(toastList()).toHaveLength(1);
    await fireEvent.focusOut(skip, { relatedTarget: document.body });
    await vi.advanceTimersByTimeAsync(4010);
    expect(toastList()).toHaveLength(0);
  });
});
