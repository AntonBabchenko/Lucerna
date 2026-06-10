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

  it('a backdrop click calls onClose; a panel click does not', async () => {
    const onClose = vi.fn();
    const { getByRole } = render(Modal, {
      props: { onClose, ariaLabel: 'x', children: body() },
    });
    const dialog = getByRole('dialog');
    const backdrop = dialog.parentElement as HTMLElement;

    // Click on the panel — must NOT close (target !== backdrop).
    await fireEvent.click(dialog);
    expect(onClose).not.toHaveBeenCalled();

    // Click directly on the backdrop — closes.
    await fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('backdrop click is ignored when closeOnBackdrop is false', async () => {
    const onClose = vi.fn();
    const { getByRole } = render(Modal, {
      props: { onClose, ariaLabel: 'x', closeOnBackdrop: false, children: body() },
    });
    const backdrop = getByRole('dialog').parentElement as HTMLElement;
    await fireEvent.click(backdrop);
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
