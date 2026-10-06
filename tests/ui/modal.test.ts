import { createEvent, fireEvent, render } from '@testing-library/svelte';
import { createRawSnippet, tick } from 'svelte';
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

  it('a press on the backdrop does not move the focus; a press on the panel does', async () => {
    // Left to the browser, a press on the backdrop (not focusable) would drop the
    // focus to <body>, and the user's place in the dialog with it.
    const { getByRole } = render(Modal, {
      props: { onClose: vi.fn(), ariaLabel: 'x', children: body() },
    });
    const dialog = getByRole('dialog');
    const backdrop = dialog.parentElement as HTMLElement;

    const onBackdrop = createEvent.mouseDown(backdrop);
    await fireEvent(backdrop, onBackdrop);
    expect(onBackdrop.defaultPrevented).toBe(true);

    const onPanel = createEvent.mouseDown(dialog);
    await fireEvent(dialog, onPanel);
    expect(onPanel.defaultPrevented).toBe(false);
  });

  it('a backdrop click leaves the focused control before it closes', async () => {
    // Since the press keeps the focus, the dismissal blurs it: a field that saves
    // when it is left (Manage's name) saves, then the dialog closes.
    const order: string[] = [];
    const { getByRole } = render(Modal, {
      props: { onClose: () => order.push('close'), ariaLabel: 'x', children: body() },
    });
    const dialog = getByRole('dialog');
    const backdrop = dialog.parentElement as HTMLElement;
    const ok = getByRole('button', { name: 'OK' });
    ok.focus();
    ok.addEventListener('blur', () => order.push('blur'));

    await fireEvent.mouseDown(backdrop);
    expect(document.activeElement).toBe(ok);
    await fireEvent.mouseUp(backdrop);
    expect(order).toEqual(['blur', 'close']);
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

  // A dialog that swaps its content (a step, a phase) loses the focused control with the old
  // content, and the focus falls to <body> (here: blur() — the state a removal leaves). The panel
  // takes it on a step change — never a focus something else holds, and never for a lower dialog.
  describe('stepKey', () => {
    const props = (stepKey: unknown) => ({
      onClose: vi.fn(),
      ariaLabel: 'x',
      stepKey,
      children: body(),
    });

    it('puts a focus lost to <body> on the panel when the step changes', async () => {
      const { getByRole, rerender } = render(Modal, { props: props('pick') });
      getByRole('button', { name: 'OK' }).focus();
      (document.activeElement as HTMLElement).blur();
      await rerender(props('review'));
      await tick();
      expect(document.activeElement).toBe(getByRole('dialog'));
    });

    it('leaves a focus that something else holds', async () => {
      const { rerender } = render(Modal, { props: props('pick') });
      const toastButton = document.createElement('button');
      document.body.appendChild(toastButton);
      toastButton.focus();
      await rerender(props('review'));
      await tick();
      expect(document.activeElement).toBe(toastButton);
      toastButton.remove();
    });

    it('does nothing while the step stays the same', async () => {
      const { getByRole, rerender } = render(Modal, { props: props('pick') });
      getByRole('button', { name: 'OK' }).focus();
      (document.activeElement as HTMLElement).blur();
      await rerender(props('pick'));
      await tick();
      expect(document.activeElement).toBe(document.body);
    });

    it('does nothing for a dialog under another one', async () => {
      const lower = render(Modal, { props: props('pick') });
      render(Modal, { props: { onClose: vi.fn(), ariaLabel: 'top', children: body('top') } });
      (document.activeElement as HTMLElement | null)?.blur();
      await lower.rerender(props('review'));
      await tick();
      expect(document.activeElement).toBe(document.body);
    });
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
