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
});
