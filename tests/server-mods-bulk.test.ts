// The server mods pane's bulk bar: the same gesture as the client Installed list, over the
// filtered rows, calling the single-item commands with the ON-DISK filename, asking before a
// removal, and off entirely while the server runs.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerCore } from '$lib/ipc/bindings';
import ServerModsInstalled from '$lib/servers/addons/ServerModsInstalled.svelte';

const {
  mockListEnriched,
  mockEnrich,
  mockProjects,
  mockDeleteMod,
  mockEnableMod,
  mockDisableMod,
  mockOpenFolder,
  mockQuarantine,
  mockCheckUpdates,
  mockUpdateOne,
  toasts,
  serverRow,
} = vi.hoisted(() => {
  const serverRow = {
    id: 'srv-1',
    name: 'My Server',
    mc_version: '1.20.1',
    loader: 'forge' as ServerCore,
    loader_version: '47.4.0' as string | null,
    max_heap_mb: 4096,
    extra_jvm_args: '',
    created_unix_ms: 1 as number | null,
    eula_accepted: true,
    created_from_instance: null as string | null,
    running: false,
    pid: null as number | null,
    port: null as number | null,
    upload: null,
    upload_password_set: false,
  };
  return {
    mockListEnriched: vi.fn(),
    mockEnrich: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    mockProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockDeleteMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnableMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockDisableMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockOpenFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockQuarantine: vi.fn(),
    mockCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockUpdateOne: vi.fn(),
    toasts: { pushSuccess: vi.fn(), pushWarning: vi.fn() },
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListModsEnriched: mockListEnriched,
    serverEnrichMods: mockEnrich,
    modsProjects: mockProjects,
    serverDeleteMod: mockDeleteMod,
    serverEnableMod: mockEnableMod,
    serverDisableMod: mockDisableMod,
    serverOpenFolder: mockOpenFolder,
    serverCheckModUpdates: mockCheckUpdates,
    serverUpdateOne: mockUpdateOne,
  },
}));
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [serverRow];
    },
    quarantineClientMods: mockQuarantine,
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

const modRow = (filename: string, opts: { disabled?: boolean } = {}) => ({
  filename,
  on_disk_filename: opts.disabled ? `${filename}.disabled` : filename,
  disabled: opts.disabled ?? false,
  reason: null,
  sha1: filename,
  source: null,
  project_id: null,
  version_id: null,
  name: null,
  version_number: null,
});

async function mountWith(rows: ReturnType<typeof modRow>[]) {
  mockListEnriched.mockResolvedValue({ status: 'ok', data: rows });
  render(ServerModsInstalled, { props: { serverId: 'srv-1' } });
  await screen.findByTestId('bulk-select-all');
}

// The row checkboxes only: the bar's own «Select all» box is labelled «Select all» and would
// otherwise come first in document order.
const rowCheckboxes = () => screen.getAllByRole('checkbox', { name: /^select (?!all$)/i });

beforeEach(() => {
  vi.clearAllMocks();
  serverRow.running = false;
  mockListEnriched.mockReset();
});

describe('ServerModsInstalled — bulk actions', () => {
  it('Select all counts the rows; Disable flips only the enabled ones, by on-disk filename', async () => {
    await mountWith([modRow('a.jar'), modRow('b.jar', { disabled: true })]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    expect(screen.getByText(/2 selected/)).toBeTruthy();
    const bar = screen.getByTestId('bulk-bar');
    await fireEvent.click(within(bar).getByRole('button', { name: 'Disable' }));
    await waitFor(() => expect(mockDisableMod).toHaveBeenCalledTimes(1));
    expect(mockDisableMod).toHaveBeenCalledWith('srv-1', 'a.jar');
    expect(mockEnableMod).not.toHaveBeenCalled();
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Disabled 1 of 1');
  });

  it('Enable uses the .disabled on-disk name and is off when no selected row is disabled', async () => {
    await mountWith([modRow('a.jar'), modRow('b.jar', { disabled: true })]);
    const boxes = rowCheckboxes();
    await fireEvent.click(boxes[0]); // a.jar, enabled
    const bar = screen.getByTestId('bulk-bar');
    expect(within(bar).getByRole('button', { name: 'Enable' }).hasAttribute('disabled')).toBe(true);
    await fireEvent.click(boxes[1]); // + b.jar, disabled
    await fireEvent.click(within(bar).getByRole('button', { name: 'Enable' }));
    await waitFor(() => expect(mockEnableMod).toHaveBeenCalledWith('srv-1', 'b.jar.disabled'));
    expect(mockEnableMod).toHaveBeenCalledTimes(1);
  });

  it('Remove asks with the names and deletes nothing until confirmed, then deletes each by on-disk name', async () => {
    await mountWith([modRow('a.jar'), modRow('b.jar', { disabled: true })]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Remove 2 mods?')).toBeTruthy();
    expect(within(dialog).getByText('a.jar')).toBeTruthy();
    expect(mockDeleteMod).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByTestId('server-mods-bulk-delete-confirm'));
    await waitFor(() => expect(mockDeleteMod).toHaveBeenCalledTimes(2));
    expect(mockDeleteMod).toHaveBeenNthCalledWith(1, 'srv-1', 'a.jar');
    expect(mockDeleteMod).toHaveBeenNthCalledWith(2, 'srv-1', 'b.jar.disabled');
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Removed 2 of 2');
  });

  it('a partial failure is one warning naming the reason, and the run goes on', async () => {
    mockDeleteMod
      .mockResolvedValueOnce({ status: 'error', error: { kind: 'io', message: 'locked' } })
      .mockResolvedValueOnce({ status: 'ok', data: null });
    await mountWith([modRow('a.jar'), modRow('b.jar')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    await fireEvent.click(await screen.findByTestId('server-mods-bulk-delete-confirm'));
    await waitFor(() => expect(mockDeleteMod).toHaveBeenCalledTimes(2));
    expect(toasts.pushWarning).toHaveBeenCalledTimes(1);
    expect(toasts.pushWarning.mock.calls[0][0]).toBe('Removed 1 of 2, 1 failed');
    expect(toasts.pushWarning.mock.calls[0][1]).toHaveLength(1);
  });

  it('while the server runs every bar action is off and says to stop the server', async () => {
    serverRow.running = true;
    await mountWith([modRow('a.jar')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    for (const name of ['Enable', 'Disable', 'Update', 'Remove']) {
      const btn = within(bar).getByRole('button', { name });
      expect(btn.hasAttribute('disabled')).toBe(true);
      expect(btn.parentElement?.getAttribute('tabindex')).toBe('0');
    }
    expect(within(bar).getByRole('button', { name: /clear/i }).hasAttribute('disabled')).toBe(
      false,
    );
  });

  it('while a bulk run is in flight the rows and the toolbar wait for it', async () => {
    let finish: (v: unknown) => void = () => {};
    mockDisableMod.mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
    await mountWith([modRow('a.jar'), modRow('b.jar')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Disable' }),
    );
    await waitFor(() => expect(mockDisableMod).toHaveBeenCalledTimes(1));
    // The mods folder is being written: nothing else may write it meanwhile.
    for (const id of [
      'server-mods-check-updates',
      'server-mods-update-all',
      'server-mods-quarantine',
    ])
      expect(screen.getByTestId(id).hasAttribute('disabled')).toBe(true);
    for (const row of document.querySelectorAll<HTMLElement>('[data-bulk-row]'))
      expect(within(row).getByTestId('card-actions-blocked')).toBeTruthy();
    finish({ status: 'ok', data: null });
    await waitFor(() => expect(toasts.pushSuccess).toHaveBeenCalledWith('Disabled 2 of 2'));
    expect(screen.getByTestId('server-mods-check-updates').hasAttribute('disabled')).toBe(false);
    expect(document.querySelector('[data-testid="card-actions-blocked"]')).toBeNull();
  });
});
