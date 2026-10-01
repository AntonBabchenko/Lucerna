import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const { importInspect, importCommit, importCancel, memBounds } = vi.hoisted(() => ({
  importInspect: vi.fn(),
  // `server` is required on importCommit's success arm (the store now returns a
  // discriminated union), and doImport reads `r.server.id` without a hedge.
  importCommit: vi.fn().mockResolvedValue({ ok: true, server: { id: 'srv-imported' } }),
  importCancel: vi.fn().mockResolvedValue(undefined),
  memBounds: vi.fn().mockResolvedValue({
    min_mb: 1024,
    max_mb: 8192,
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instanceMemoryBounds: memBounds,
    // LoaderPicker fires async loader-version fetches whenever mc+loader change.
    // Return a list containing the preview's loader_version so LoaderPicker
    // preserves the parent's pick instead of resetting it to null.
    listFabricLoaders: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: [{ version: '0.16.5', stable: true }] }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
}));
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { importInspect, importCommit, importCancel },
}));
// Stub the Tauri dialog so the "Choose .zip" button resolves a path.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue('C:/x/MyServer.zip'),
}));

import ServerImportView from '$lib/servers/ServerImportView.svelte';

const preview = (over = {}) => ({
  token: 't1',
  detected_name: 'MyServer',
  mc_version: '1.20.4',
  loader: 'fabric',
  loader_version: '0.16.5',
  can_launch_as_is: true,
  mod_count: 3,
  world_present: true,
  eula_in_source: true,
  size_bytes: 1000,
  ...over,
});

describe('ServerImportView', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    importInspect.mockReset();
    importCommit.mockClear();
    importCancel.mockClear();
  });

  async function openConfirm(over = {}) {
    importInspect.mockResolvedValue({ ok: true, preview: preview(over) });
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose .zip' }));
    return (await screen.findByLabelText('Minecraft version')) as HTMLInputElement;
  }

  const importButton = () => screen.getByRole('button', { name: 'Import' }) as HTMLButtonElement;

  it('blocks Import and says why when the Minecraft version was not detected', async () => {
    const input = await openConfirm({ mc_version: null });
    expect(importButton().disabled).toBe(true);
    expect(screen.getByTestId('import-disabled-reason').textContent).toContain(
      "Enter the server's Minecraft version to continue.",
    );
    const hint = screen.getByText(/couldn't read this server's Minecraft version/);
    expect(input.getAttribute('aria-describedby')).toBe(hint.id);

    await fireEvent.input(input, { target: { value: '1.20.4' } });
    expect(importButton().disabled).toBe(false);
    expect(screen.queryByTestId('import-disabled-reason')).toBeNull();
  });

  // The UI does not judge a filled-in version against the list it loaded at
  // startup (a snapshot released since would be refused falsely); the backend
  // checks Mojang's current list and its refusal is shown.
  it("shows the backend's refusal of a version Mojang does not list", async () => {
    importCommit.mockResolvedValueOnce({
      ok: false,
      error: { kind: 'server_mc_version_unlisted', mc_version: '1.20.l' },
    });
    await openConfirm({ mc_version: '1.20.l' });
    expect(importButton().disabled).toBe(false);
    expect(screen.queryByTestId('import-disabled-reason')).toBeNull();
    await fireEvent.click(importButton());
    await screen.findByText(/Minecraft 1\.20\.l is not in Mojang's version list/);
  });

  it('sends the trimmed Minecraft version', async () => {
    const input = await openConfirm({ mc_version: null });
    await fireEvent.input(input, { target: { value: '  1.20.4  ' } });
    await fireEvent.click(importButton());
    await waitFor(() => expect(importCommit).toHaveBeenCalled());
    expect(importCommit.mock.calls[0][2]).toBe('1.20.4');
  });

  it('after choosing a zip, inspects and shows the prefilled confirm step', async () => {
    importInspect.mockResolvedValue({ ok: true, preview: preview() });
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose .zip' }));
    await waitFor(() => expect(importInspect).toHaveBeenCalledWith('C:/x/MyServer.zip'));
    // Name prefilled
    const nameInput = (await screen.findByLabelText('Name')) as HTMLInputElement;
    expect(nameInput.value).toBe('MyServer');
  });

  it('Import calls importCommit with the confirmed fields then onDone', async () => {
    importInspect.mockResolvedValue({ ok: true, preview: preview() });
    const onDone = vi.fn();
    render(ServerImportView, { onDone, onCancel: vi.fn() });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose .zip' }));
    await screen.findByLabelText('Name');
    await fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    await waitFor(() =>
      expect(importCommit).toHaveBeenCalledWith(
        't1',
        'MyServer',
        '1.20.4',
        'fabric',
        '0.16.5',
        expect.any(Number),
        true,
      ),
    );
    // The created server's id is handed back so the panel can select it. Under
    // the old flattened return type this was `onDone(r.server?.id)` and an
    // absent server silently became `undefined`; the union guarantees it.
    await waitFor(() => expect(onDone).toHaveBeenCalledWith('srv-imported'));
  });

  it('warns when the loader is undetected and defaults to Vanilla (#20)', async () => {
    importInspect.mockResolvedValue({
      ok: true,
      preview: preview({ loader: null, loader_version: null }),
    });
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose .zip' }));
    await screen.findByTestId('import-loader-unknown-warn');
  });

  it('does not warn when a loader was detected', async () => {
    importInspect.mockResolvedValue({ ok: true, preview: preview() });
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose .zip' }));
    await screen.findByLabelText('Name');
    expect(screen.queryByTestId('import-loader-unknown-warn')).toBeNull();
  });

  it('rejects a non-server source with the error message', async () => {
    importInspect.mockResolvedValue({ ok: false, error: { kind: 'server_import_not_a_server' } });
    render(ServerImportView, { onDone: vi.fn(), onCancel: vi.fn() });
    await fireEvent.click(screen.getByRole('button', { name: 'Choose .zip' }));
    await waitFor(() =>
      expect(screen.getByText(/doesn't look like a Minecraft server/i)).toBeTruthy(),
    );
  });
});
