// Toasts sit at the top right and above everything, a modal included: the
// maintainer asked for one place (2026-10-07), overriding UPD-17, which moved
// the stack to the bottom centre while a modal was open.
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
import { modalDepth } from '$lib/ui/layer-stack.svelte';
import Modal from '$lib/ui/Modal.svelte';

const body = createRawSnippet(() => ({
  render: () => '<div><h2 id="over-toasts-title">Title</h2><button>OK</button></div>',
}));

function clearToasts() {
  for (const t of [...toastList()]) dismiss(t.id);
}
beforeEach(clearToasts);
afterEach(clearToasts);

describe('toasts while a modal is open', () => {
  it('stay at the top right, above the modal, while it is open and after it closes', async () => {
    pushInfo('Heads up');
    const host = render(ToastHost).getByTestId('toast-host');
    const atTopRight = () => {
      expect(host.className).toContain('top-4');
      expect(host.className).toContain('right-4');
      expect(host.className).not.toContain('bottom-4');
      expect(host.className).not.toContain('left-1/2');
      // Above every modal (--z-modal 50) and tour (--z-tour 100).
      expect(host.className).toContain('z-[var(--z-toast)]');
    };
    expect(modalDepth()).toBe(0);
    atTopRight();

    const modal = render(Modal, {
      props: { onClose: vi.fn(), ariaLabelledby: 'over-toasts-title', children: body },
    });
    await tick();
    expect(modalDepth()).toBe(1);
    atTopRight();

    modal.unmount();
    await tick();
    expect(modalDepth()).toBe(0);
    atTopRight();
  });
});
