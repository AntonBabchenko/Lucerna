import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';

describe('ModpacksModal', () => {
  it('renders nothing when open=false', () => {
    const { container } = render(ModpacksModal, {
      props: { open: false, onClose: () => {} },
    });
    expect(container.querySelector('[data-testid="modpacks-modal"]')).toBeNull();
  });

  it('renders a dialog titled "Modpacks" with a single Close control when open', () => {
    render(ModpacksModal, { props: { open: true, onClose: () => {} } });
    const dialog = screen.getByRole('dialog', { name: /modpacks/i });
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    // Only the × remains; the redundant "← Back" control was removed.
    expect(screen.getByLabelText('Close modpacks')).toHaveBtnVariant('icon');
    expect(screen.queryByTestId('modpacks-modal-back')).toBeNull();
  });

  it('×, scrim, and Escape each call onClose', async () => {
    const onClose = vi.fn();
    render(ModpacksModal, { props: { open: true, onClose } });
    await fireEvent.click(screen.getByLabelText('Close modpacks'));
    // The scrim is the full-bleed backdrop button, labelled simply "Close".
    await fireEvent.click(screen.getByLabelText('Close'));
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});
