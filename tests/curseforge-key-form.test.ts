import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// The three IPC commands the form drives are wrapped in typedError on
// the real bindings, so they resolve to a `{ status: 'ok' | 'error' }`
// envelope rather than throwing. Each test mocks them with that shape.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn(),
    modsSetCurseforgeKey: vi.fn(),
    modsClearCurseforgeKey: vi.fn(),
  },
}));

import CurseForgeKeyForm from '$lib/settings/CurseForgeKeyForm.svelte';

describe('CurseForgeKeyForm', () => {
  it('shows Missing → Set after Save with a valid key', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsGetCurseforgeKeyStatus as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce({ status: 'ok', data: 'missing' })
      .mockResolvedValueOnce({ status: 'ok', data: 'set' });
    (mod.commands.modsSetCurseforgeKey as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });

    render(CurseForgeKeyForm);
    // Yield once so the mount-time refresh() promise resolves before we
    // assert on the rendered status text.
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/Not configured/)).toBeTruthy();

    const input = screen.getByPlaceholderText(/\$2a/i) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'good-key' } });
    await fireEvent.click(screen.getByRole('button', { name: /Save key|Update key/ }));
    // Save chains modsSetCurseforgeKey → refresh(). Yield twice so both
    // microtasks settle before asserting on the post-save status text.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));

    expect(mod.commands.modsSetCurseforgeKey).toHaveBeenCalledWith('good-key');
    expect(screen.getByText(/OK — key is set/)).toBeTruthy();
  });

  it('flips to Invalid when modsSetCurseforgeKey returns an error', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsGetCurseforgeKeyStatus as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: 'missing',
    });
    (mod.commands.modsSetCurseforgeKey as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'mods_platform_auth', kind_detail: 'invalid' },
    });

    render(CurseForgeKeyForm);
    await new Promise((r) => setTimeout(r, 0));

    const input = screen.getByPlaceholderText(/\$2a/i) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'bad-key' } });
    await fireEvent.click(screen.getByRole('button', { name: /Save key|Update key/ }));
    await new Promise((r) => setTimeout(r, 0));

    expect(screen.getByText(/Invalid — please enter a new key/)).toBeTruthy();
  });

  it('a key CurseForge accepted but the keyring refused to keep is "not saved", never "Invalid"', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsGetCurseforgeKeyStatus as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: 'missing',
    });
    (mod.commands.modsSetCurseforgeKey as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'keyring', op: 'write', details: 'Platform secure storage failure: locked' },
    });

    render(CurseForgeKeyForm);
    await new Promise((r) => setTimeout(r, 0));

    const input = screen.getByPlaceholderText(/\$2a/i) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'good-key' } });
    await fireEvent.click(screen.getByRole('button', { name: /Save key|Update key/ }));
    await new Promise((r) => setTimeout(r, 0));

    expect(screen.getByText(/Accepted by CurseForge, but not saved/)).toBeTruthy();
    expect(screen.queryByText(/Invalid — please enter a new key/)).toBeNull();
    // The keyring's own reason reaches the user under the headline.
    expect(screen.getByRole('alert').textContent).toContain('locked');
  });

  it('a status the keyring could not answer says so and Retry re-reads it', async () => {
    const mod = await import('$lib/ipc/bindings');
    // The mock is shared across this file: start its call count from zero.
    (mod.commands.modsGetCurseforgeKeyStatus as ReturnType<typeof vi.fn>)
      .mockReset()
      .mockResolvedValueOnce({ status: 'ok', data: 'unknown' })
      .mockResolvedValueOnce({ status: 'ok', data: 'set' });

    render(CurseForgeKeyForm);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/Couldn't check the system keyring/)).toBeTruthy();
    expect(screen.queryByText(/Not configured/)).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: /Check again/ }));
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.modsGetCurseforgeKeyStatus).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/OK — key is set/)).toBeTruthy();
  });
});
