// The CurseForge key form keeps two facts apart: what is STORED (the status
// line, from the status command) and what HAPPENED to what was just typed
// (the result under the field). INT-01 / INT-02 / INT-12 / INT-16.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The three IPC commands the form drives are wrapped in typedError on the real
// bindings, so they resolve to a `{ status: 'ok' | 'error' }` envelope rather
// than throwing. Each test mocks them with that shape.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn(),
    modsSetCurseforgeKey: vi.fn(),
    modsClearCurseforgeKey: vi.fn(),
  },
}));

import { commands } from '$lib/ipc/bindings';
import CurseForgeKeyForm from '$lib/settings/CurseForgeKeyForm.svelte';
import { cfKeyVersion } from '$lib/settings/state.svelte';

const status = vi.mocked(commands.modsGetCurseforgeKeyStatus);
const setKey = vi.mocked(commands.modsSetCurseforgeKey);
const clearKey = vi.mocked(commands.modsClearCurseforgeKey);

const REJECTED = { kind: 'mods_platform_auth', kind_detail: 'invalid' } as const;

async function paste(value: string) {
  const input = (await screen.findByTestId('cf-key-input')) as HTMLInputElement;
  await waitFor(() => expect(input.disabled).toBe(false));
  await fireEvent.input(input, { target: { value } });
  await fireEvent.click(screen.getByTestId('cf-key-save'));
}

describe('CurseForgeKeyForm', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    status.mockResolvedValue({ status: 'ok', data: 'missing' });
    setKey.mockResolvedValue({ status: 'ok', data: null });
    clearKey.mockResolvedValue({ status: 'ok', data: null });
  });

  it('(a) on a build with its own key: names it, offers no Clear, folds the own-key path away', async () => {
    status.mockResolvedValue({ status: 'ok', data: 'set_builtin' });
    render(CurseForgeKeyForm);
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain(
        "Using Lucerna's built-in key",
      ),
    );
    expect(screen.queryByTestId('cf-key-clear')).toBeNull();
    const details = screen.getByTestId('cf-key-own-key') as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(details.textContent).toContain('Generate a key for Lucerna');
    expect(details.contains(screen.getByTestId('cf-key-input'))).toBe(true);
    expect(screen.getByTestId('cf-key-save').textContent).toContain('Save key');
  });

  it('(b) a rejected candidate never relabels a stored own key, and does not re-arm the banners', async () => {
    status.mockResolvedValue({ status: 'ok', data: 'set' });
    setKey.mockResolvedValueOnce({ status: 'error', error: REJECTED });
    const before = cfKeyVersion.value;
    render(CurseForgeKeyForm);
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain('Your own key is stored'),
    );
    await paste('typo');
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-result').textContent).toContain(
        'Rejected by CurseForge — not saved',
      ),
    );
    expect(screen.getByTestId('cf-key-status').textContent).toContain('Your own key is stored');
    expect(screen.queryByText(/Generate a key for Lucerna/)).toBeNull();
    expect(cfKeyVersion.value).toBe(before);
  });

  it('(c) with no key at all, a rejected candidate keeps the steps and the "Key" label', async () => {
    setKey.mockResolvedValueOnce({ status: 'error', error: REJECTED });
    render(CurseForgeKeyForm);
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain('No key'),
    );
    await paste('typo');
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-result').textContent).toContain('Rejected by CurseForge'),
    );
    expect(screen.getByText(/Generate a key for Lucerna/)).toBeTruthy();
    // The exact label — "New key (replaces your own key)" would not match.
    expect(screen.getByText('Key')).toBeTruthy();
    expect(screen.queryByTestId('cf-key-clear')).toBeNull();
  });

  it('(d) Clear removes only your own key and the status says what serves next', async () => {
    status
      .mockResolvedValueOnce({ status: 'ok', data: 'set' })
      .mockResolvedValueOnce({ status: 'ok', data: 'set_builtin' });
    const before = cfKeyVersion.value;
    render(CurseForgeKeyForm);
    const clear = await screen.findByTestId('cf-key-clear');
    expect(screen.getByText('Removes your own key.')).toBeTruthy();
    await fireEvent.click(clear);
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain(
        "Using Lucerna's built-in key",
      ),
    );
    expect(clearKey).toHaveBeenCalledTimes(1);
    expect(cfKeyVersion.value).toBe(before + 1);
  });

  it('(e) while the status loads nothing presumes a stored key', () => {
    status.mockReturnValue(new Promise(() => {}));
    render(CurseForgeKeyForm);
    expect(screen.getByTestId('cf-key-status').textContent).toContain('Checking…');
    expect((screen.getByTestId('cf-key-input') as HTMLInputElement).disabled).toBe(true);
    expect(screen.queryByTestId('cf-key-clear')).toBeNull();
    expect(screen.queryByText(/replaces your own key/)).toBeNull();
    expect(screen.getByTestId('cf-key-save').textContent).toContain('Save key');
  });

  it("(f) a failed Save's message disappears when Clear starts", async () => {
    status.mockResolvedValue({ status: 'ok', data: 'set' });
    setKey.mockResolvedValueOnce({ status: 'error', error: REJECTED });
    clearKey.mockReturnValueOnce(new Promise(() => {}));
    render(CurseForgeKeyForm);
    await paste('typo');
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-result').textContent).toContain('Rejected'),
    );
    await fireEvent.click(screen.getByTestId('cf-key-clear'));
    await waitFor(() => expect(screen.getByTestId('cf-key-result').textContent?.trim()).toBe(''));
  });

  it('(g) an unreachable CurseForge is "not saved", and the stored state is untouched', async () => {
    setKey.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'mods_platform_unreachable', url: 'https://api.curseforge.com/v1/games/432' },
    });
    render(CurseForgeKeyForm);
    await paste('maybe-good');
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-result').textContent).toContain(
        "Couldn't reach CurseForge — not saved",
      ),
    );
    expect(screen.getByTestId('cf-key-status').textContent).toContain('No key');
    expect(status).toHaveBeenCalledTimes(1);
  });

  it('(h) a status command that fails says so, with the reason, and Check again re-reads', async () => {
    status
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'io', path: '<mods_get_curseforge_key_status>', details: 'join: cancelled' },
      })
      .mockResolvedValueOnce({ status: 'ok', data: 'set' });
    render(CurseForgeKeyForm);
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain(
        "Couldn't check the system keyring",
      ),
    );
    expect(screen.getByTestId('cf-key-status-reason').textContent).toContain('cancelled');
    await fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain('Your own key is stored'),
    );
  });

  it('(i) a key CurseForge accepted but the keyring refused to keep is "not saved", never a rejection', async () => {
    setKey.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'keyring', op: 'write', details: 'Platform secure storage failure: locked' },
    });
    render(CurseForgeKeyForm);
    await paste('good-key');
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-result').textContent).toContain(
        'Accepted by CurseForge, but not saved',
      ),
    );
    expect(screen.getByTestId('cf-key-result').textContent).toContain('locked');
    expect(screen.queryByText(/Rejected/)).toBeNull();
  });

  it('(j) a saved key says so and re-arms the banners once', async () => {
    status
      .mockResolvedValueOnce({ status: 'ok', data: 'missing' })
      .mockResolvedValueOnce({ status: 'ok', data: 'set' });
    const before = cfKeyVersion.value;
    render(CurseForgeKeyForm);
    await paste('good-key');
    await waitFor(() =>
      expect(screen.getByTestId('cf-key-status').textContent).toContain('Your own key is stored'),
    );
    expect(screen.getByTestId('cf-key-result').textContent).toContain('Saved');
    expect(setKey).toHaveBeenCalledWith('good-key');
    expect(cfKeyVersion.value).toBe(before + 1);
  });
});
