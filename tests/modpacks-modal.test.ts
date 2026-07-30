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

  it('shows a (?) help explaining why a modpack install creates a new instance', async () => {
    render(ModpacksModal, { props: { open: true, onClose: () => {} } });
    const help = screen.getByRole('button', {
      name: /why does installing a modpack create a new instance/i,
    });
    await fireEvent.click(help);
    expect(screen.getByText(/makes its own new instance/i)).toBeTruthy();
  });

  it('×, scrim, and Escape each call onClose', async () => {
    const onClose = vi.fn();
    render(ModpacksModal, { props: { open: true, onClose } });
    await fireEvent.click(screen.getByLabelText('Close modpacks'));
    // The shared Modal renders the scrim as the dialog's parent backdrop div;
    // a press AND release both on it (target === backdrop) dismisses.
    const dialog = screen.getByRole('dialog', { name: /modpacks/i });
    const scrim = dialog.parentElement as HTMLElement;
    await fireEvent.mouseDown(scrim);
    await fireEvent.mouseUp(scrim);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it('states in the header that installing a pack creates a new instance', () => {
    render(ModpacksModal, { props: { open: true, onClose: () => {} } });
    expect(screen.getByTestId('modpacks-scope-note').textContent).toContain(
      'creates a new instance',
    );
  });
});
