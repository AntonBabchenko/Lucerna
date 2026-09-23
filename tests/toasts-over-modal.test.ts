// UPD-17. While a modal is open its panel is vertically centred: the header ×
// is not at the viewport top and the footer's primary action sits bottom
// right, so a toast at the top right covers the one control the user is about
// to use (at 820×520 the Settings header spans ≈52–97 px). The stack moves to
// the bottom centre for as long as any modal is open, newest nearest the
// edge, and back to the top right when the last one unmounts.
import { render } from '@testing-library/svelte';
import { createRawSnippet, tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// ToastHost registers a Tauri event listener on mount; Modal imports nothing
// from the bindings.
vi.mock('$lib/ipc/bindings', () => ({
  events: {
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ToastHost from '$lib/toasts/ToastHost.svelte';
import { dismiss, pushInfo, toastList } from '$lib/toasts/toasts.svelte';
import Modal, { modalDepth } from '$lib/ui/Modal.svelte';

const body = createRawSnippet(() => ({
  render: () => '<div><h2 id="over-toasts-title">Title</h2><button>OK</button></div>',
}));

function clearToasts() {
  for (const t of [...toastList()]) dismiss(t.id);
}
beforeEach(clearToasts);
afterEach(clearToasts);

describe('toasts while a modal is open', () => {
  it('sit at the bottom centre while a modal is open and return to the top right after it closes', async () => {
    pushInfo('Heads up');
    const host = render(ToastHost).getByTestId('toast-host');
    expect(modalDepth()).toBe(0);
    expect(host.className).toContain('top-4');
    expect(host.className).not.toContain('bottom-4');

    const modal = render(Modal, {
      props: { onClose: vi.fn(), ariaLabelledby: 'over-toasts-title', children: body },
    });
    await tick();
    expect(modalDepth()).toBe(1);
    expect(host.className).toContain('bottom-4');
    expect(host.className).toContain('left-1/2');
    expect(host.className).toContain('-translate-x-1/2');
    expect(host.className).toContain('flex-col-reverse');
    expect(host.className).not.toContain('top-4');

    modal.unmount();
    await tick();
    expect(modalDepth()).toBe(0);
    expect(host.className).toContain('top-4');
    expect(host.className).not.toContain('bottom-4');
  });
});
