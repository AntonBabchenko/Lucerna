import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';

describe('ModpacksModal', () => {
  it('renders nothing when open=false', () => {
    const { container } = render(ModpacksModal, {
      props: { open: false, importing: false, onClose: () => {} },
    });
    expect(container.querySelector('[data-testid="modpacks-modal"]')).toBeNull();
  });

  it('renders a dialog titled "Modpacks" with Back and Close when open', () => {
    render(ModpacksModal, { props: { open: true, importing: false, onClose: () => {} } });
    const dialog = screen.getByRole('dialog', { name: /modpacks/i });
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    const back = screen.getByTestId('modpacks-modal-back');
    expect(back).toHaveBtnVariant('secondary');
    expect(back).toHaveBtnSize('sm');
    expect(back.textContent).toContain('Back');
    expect(screen.getByLabelText('Close modpacks')).toHaveBtnVariant('icon');
  });

  it('Back, ×, and scrim each call onClose when not importing', async () => {
    const onClose = vi.fn();
    render(ModpacksModal, { props: { open: true, importing: false, onClose } });
    await fireEvent.click(screen.getByTestId('modpacks-modal-back'));
    await fireEvent.click(screen.getByLabelText('Close modpacks'));
    // The scrim is the full-bleed backdrop button, labelled simply "Close".
    await fireEvent.click(screen.getByLabelText('Close'));
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it('Escape calls onClose when open and not importing', async () => {
    const onClose = vi.fn();
    render(ModpacksModal, { props: { open: true, importing: false, onClose } });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('suppresses all close paths while importing', async () => {
    const onClose = vi.fn();
    render(ModpacksModal, { props: { open: true, importing: true, onClose } });
    await fireEvent.click(screen.getByTestId('modpacks-modal-back'));
    await fireEvent.click(screen.getByLabelText('Close modpacks'));
    await fireEvent.click(screen.getByLabelText('Close'));
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });
});
