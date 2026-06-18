import { fireEvent, render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import Modal from '../../src/lib/ui/Modal.svelte';

// Children with a labelled heading + a focusable control.
const body = (id = 'modal-title') =>
  createRawSnippet(() => ({
    render: () => `<div><h2 id="${id}">Title</h2><button>OK</button></div>`,
  }));

describe('Modal', () => {
  it('renders role=dialog + aria-modal and applies ariaLabel', () => {
    const { getByRole } = render(Modal, {
      props: { onClose: vi.fn(), ariaLabel: 'Settings', children: body() },
    });
    const dialog = getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-label')).toBe('Settings');
  });

  it('wires ariaLabelledby to the heading id', () => {
    const { getByRole } = render(Modal, {
      props: { onClose: vi.fn(), ariaLabelledby: 'modal-title', children: body('modal-title') },
    });
    expect(getByRole('dialog').getAttribute('aria-labelledby')).toBe('modal-title');
  });

  it('Escape calls onClose by default', async () => {
    const onClose = vi.fn();
    render(Modal, { props: { onClose, ariaLabel: 'x', children: body() } });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('Escape is ignored when closeOnEscape is false', async () => {
    const onClose = vi.fn();
    render(Modal, {
      props: { onClose, ariaLabel: 'x', closeOnEscape: false, children: body() },
    });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });

  it('a press+release both on the backdrop closes; a press+release on the panel does not', async () => {
    const onClose = vi.fn();
    const { getByRole } = render(Modal, {
      props: { onClose, ariaLabel: 'x', children: body() },
    });
    const dialog = getByRole('dialog');
    const backdrop = dialog.parentElement as HTMLElement;

    // Press and release on the panel — must NOT close.
    await fireEvent.mouseDown(dialog);
    await fireEvent.mouseUp(dialog);
    expect(onClose).not.toHaveBeenCalled();

    // Press and release directly on the backdrop — closes.
    await fireEvent.mouseDown(backdrop);
    await fireEvent.mouseUp(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('a drag-select that starts in the panel and releases on the backdrop does NOT close', async () => {
    // Regression: selecting text inside the panel and dragging past its edge
    // releases the mouse on the backdrop. That must keep the modal open — the
    // press did not start outside.
    const onClose = vi.fn();
    const { getByRole } = render(Modal, {
      props: { onClose, ariaLabel: 'x', children: body() },
    });
    const dialog = getByRole('dialog');
    const backdrop = dialog.parentElement as HTMLElement;

    await fireEvent.mouseDown(dialog); // press begins inside the panel
    await fireEvent.mouseUp(backdrop); // release lands on the backdrop
    expect(onClose).not.toHaveBeenCalled();
  });

  it('a press on the backdrop that releases on the panel does NOT close', async () => {
    // The inverse drag: both ends must be outside the panel to dismiss.
    const onClose = vi.fn();
    const { getByRole } = render(Modal, {
      props: { onClose, ariaLabel: 'x', children: body() },
    });
    const dialog = getByRole('dialog');
    const backdrop = dialog.parentElement as HTMLElement;

    await fireEvent.mouseDown(backdrop);
    await fireEvent.mouseUp(dialog);
    expect(onClose).not.toHaveBeenCalled();
  });

  it('backdrop press+release is ignored when closeOnBackdrop is false', async () => {
    const onClose = vi.fn();
    const { getByRole } = render(Modal, {
      props: { onClose, ariaLabel: 'x', closeOnBackdrop: false, children: body() },
    });
    const backdrop = getByRole('dialog').parentElement as HTMLElement;
    await fireEvent.mouseDown(backdrop);
    await fireEvent.mouseUp(backdrop);
    expect(onClose).not.toHaveBeenCalled();
  });

  it('with two nested modals, Escape closes only the topmost', async () => {
    const onCloseBase = vi.fn();
    const onCloseTop = vi.fn();
    render(Modal, { props: { onClose: onCloseBase, ariaLabel: 'base', children: body('base') } });
    render(Modal, { props: { onClose: onCloseTop, ariaLabel: 'top', children: body('top') } });

    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onCloseTop).toHaveBeenCalledTimes(1);
    expect(onCloseBase).not.toHaveBeenCalled();
  });
});
