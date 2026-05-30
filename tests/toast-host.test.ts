import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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

  it('clicking the dismiss button removes the warning toast', async () => {
    pushWarning('failed');
    const { getByLabelText, queryByText } = render(ToastHost);
    expect(queryByText('failed')).toBeTruthy();
    await fireEvent.click(getByLabelText('Dismiss notification'));
    expect(queryByText('failed')).toBeNull();
  });
});
