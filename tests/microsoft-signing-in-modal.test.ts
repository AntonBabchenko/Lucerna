import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import MicrosoftSigningInModal from '$lib/accounts/MicrosoftSigningInModal.svelte';

describe('MicrosoftSigningInModal', () => {
  it('renders nothing when open=false', () => {
    render(MicrosoftSigningInModal, { props: { open: false } });
    expect(screen.queryByText(/Complete sign-in/i)).toBeNull();
  });

  it('renders modal when open=true and Cancel fires onCancel', async () => {
    const onCancel = vi.fn();
    render(MicrosoftSigningInModal, { props: { open: true, onCancel } });
    expect(screen.getByText(/Complete sign-in/i)).toBeTruthy();
    const cancel = screen.getByRole('button', { name: /cancel/i });
    await fireEvent.click(cancel);
    expect(onCancel).toHaveBeenCalled();
  });

  it('Escape key fires onCancel when open', async () => {
    const onCancel = vi.fn();
    render(MicrosoftSigningInModal, { props: { open: true, onCancel } });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onCancel).toHaveBeenCalled();
  });
});
