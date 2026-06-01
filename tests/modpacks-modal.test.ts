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
    expect(screen.getByLabelText(/close modpacks$/i)).toHaveBtnVariant('icon');
  });

  it('Back, ×, and scrim each call onClose when not importing', async () => {
    const onClose = vi.fn();
    render(ModpacksModal, { props: { open: true, importing: false, onClose } });
    await fireEvent.click(screen.getByTestId('modpacks-modal-back'));
    await fireEvent.click(screen.getByLabelText(/close modpacks$/i));
    // The scrim is the full-bleed button with aria-label "Close modpacks (backdrop)".
    await fireEvent.click(screen.getByLabelText(/close modpacks \(backdrop\)/i));
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
    await fireEvent.click(screen.getByLabelText(/close modpacks$/i));
    await fireEvent.click(screen.getByLabelText(/close modpacks \(backdrop\)/i));
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });
});
