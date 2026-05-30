import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import MicrosoftSignInButton from '$lib/accounts/MicrosoftSignInButton.svelte';

const beginMock = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    beginMicrosoftSignin: (...args: unknown[]) => beginMock(...args),
  },
}));

describe('MicrosoftSignInButton', () => {
  it('renders the brand-exact text and logo', () => {
    render(MicrosoftSignInButton);
    expect(screen.getByText('Sign in with Microsoft')).toBeTruthy();
    const btn = screen.getByRole('button', { name: /sign in with microsoft/i });
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.getAttribute('aria-label')).toBe('Sign in with Microsoft');
    // Logo rendered as inline SVG
    expect(btn.querySelector('svg')).not.toBeNull();
  });

  it('invokes beginMicrosoftSignin on click and calls onSignedIn for ok result', async () => {
    beginMock.mockResolvedValue({ status: 'ok', data: { id: 'ms-1', name: 'Notch' } });
    const onSignedIn = vi.fn();
    render(MicrosoftSignInButton, { props: { onSignedIn } });
    const btn = screen.getByRole('button', { name: /sign in with microsoft/i });
    await fireEvent.click(btn);
    expect(beginMock).toHaveBeenCalled();
    await new Promise((r) => setTimeout(r, 0));
    expect(onSignedIn).toHaveBeenCalledWith({ id: 'ms-1', name: 'Notch' });
  });

  it('calls onError when the command returns an error', async () => {
    beginMock.mockResolvedValue({ status: 'error', error: { kind: 'auth_pending_approval' } });
    const onError = vi.fn();
    render(MicrosoftSignInButton, { props: { onError } });
    const btn = screen.getByRole('button', { name: /sign in with microsoft/i });
    await fireEvent.click(btn);
    await new Promise((r) => setTimeout(r, 0));
    expect(onError).toHaveBeenCalledWith({ kind: 'auth_pending_approval' });
  });
});
