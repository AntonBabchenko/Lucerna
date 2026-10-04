// The plugins pane's bulk bar. Hangar-hosted targets cannot be downloaded, so they are not
// bulk-updatable (as they are not part of «Update all»).
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModVersion, ServerCore } from '$lib/ipc/bindings';
import ServerPluginsInstalled from '$lib/servers/addons/ServerPluginsInstalled.svelte';

const {
  mockListEnriched,
  mockEnrich,
  mockProjects,
  mockDeletePlugin,
  mockEnablePlugin,
  mockDisablePlugin,
  mockOpenPluginsFolder,
  mockCheckUpdates,
  mockUpdateOne,
  toasts,
  serverRow,
} = vi.hoisted(() => {
  const serverRow = {
    id: 'srv-1',
    name: 'My Server',
    mc_version: '1.20.1',
    loader: 'paper' as ServerCore,
    loader_version: null as string | null,
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
    mockDeletePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockDisablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockOpenPluginsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockUpdateOne: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { installed: [], unresolved: [] } }),
    toasts: { pushSuccess: vi.fn(), pushWarning: vi.fn() },
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListPluginsEnriched: mockListEnriched,
    serverEnrichPlugins: mockEnrich,
    modsProjects: mockProjects,
    serverDeletePlugin: mockDeletePlugin,
    serverEnablePlugin: mockEnablePlugin,
    serverDisablePlugin: mockDisablePlugin,
    serverOpenPluginsFolder: mockOpenPluginsFolder,
    serverCheckPluginUpdates: mockCheckUpdates,
    serverUpdatePluginOne: mockUpdateOne,
  },
}));
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [serverRow];
    },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);
vi.mock('$lib/ui/safe-open', () => ({ openExternalHttps: vi.fn() }));

const pluginRow = (filename: string, opts: { disabled?: boolean } = {}) => ({
  filename,
  on_disk_filename: opts.disabled ? `${filename}.disabled` : filename,
  disabled: opts.disabled ?? false,
  sha1: filename,
  source: null,
  project_id: null,
  version_id: null,
  name: null,
  version_number: null,
});

// A plugin version carries no mod-loader tag (`LoaderKind` has no plugin cores).
const version = (distribution_allowed: boolean): ModVersion => ({
  source: 'modrinth',
  project_id: 'p',
  version_id: 'v2',
  name: 'P',
  version_number: '2.0',
  mc_versions: ['1.20.1'],
  loaders: [],
  primary_file: {
    filename: 'p-2.0.jar',
    url: distribution_allowed ? 'https://cdn.example/p-2.0.jar' : 'https://hangar.example/p',
    sha1: 'b',
    size: 1,
    distribution_allowed,
  },
  deps: [],
  published_at: null,
});

async function mountWith(rows: ReturnType<typeof pluginRow>[]) {
  mockListEnriched.mockResolvedValue({ status: 'ok', data: rows });
  render(ServerPluginsInstalled, { props: { serverId: 'srv-1' } });
  await screen.findByTestId('bulk-select-all');
}

beforeEach(() => {
  vi.clearAllMocks();
  serverRow.running = false;
  mockListEnriched.mockReset();
});

describe('ServerPluginsInstalled — bulk actions', () => {
  it('Enable flips only the disabled plugins, by their .disabled on-disk name', async () => {
    await mountWith([pluginRow('a.jar'), pluginRow('b.jar', { disabled: true })]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Enable' }),
    );
    await waitFor(() => expect(mockEnablePlugin).toHaveBeenCalledWith('srv-1', 'b.jar.disabled'));
    expect(mockEnablePlugin).toHaveBeenCalledTimes(1);
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Enabled 1 of 1');
  });

  it('Update applies only the selected, auto-updatable pending updates (a Hangar-hosted one is skipped)', async () => {
    mockCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [
        { sha1: 'a.jar', state: { kind: 'update_available', target: version(true) } },
        { sha1: 'b.jar', state: { kind: 'update_available', target: version(false) } },
      ],
    });
    await mountWith([pluginRow('a.jar'), pluginRow('b.jar')]);
    await fireEvent.click(screen.getByTestId('server-plugins-check-updates'));
    await waitFor(() => expect(mockCheckUpdates).toHaveBeenCalled());
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Update' }),
    );
    await waitFor(() => expect(mockUpdateOne).toHaveBeenCalledTimes(1));
    expect(mockUpdateOne).toHaveBeenCalledWith('srv-1', 'a.jar', version(true));
  });

  it('Remove asks, then deletes each by on-disk name', async () => {
    await mountWith([pluginRow('a.jar'), pluginRow('b.jar', { disabled: true })]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Remove 2 plugins?')).toBeTruthy();
    expect(mockDeletePlugin).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByTestId('server-plugins-bulk-delete-confirm'));
    await waitFor(() => expect(mockDeletePlugin).toHaveBeenCalledTimes(2));
    expect(mockDeletePlugin).toHaveBeenCalledWith('srv-1', 'b.jar.disabled');
  });

  it('while the server runs every bar action is off with the stop reason', async () => {
    serverRow.running = true;
    await mountWith([pluginRow('a.jar')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    for (const name of ['Enable', 'Disable', 'Update', 'Remove'])
      expect(within(bar).getByRole('button', { name }).hasAttribute('disabled')).toBe(true);
  });
});
