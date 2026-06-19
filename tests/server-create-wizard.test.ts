import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import ServerCreateWizard from '$lib/servers/ServerCreateWizard.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverCreate: vi.fn().mockResolvedValue({ status: 'ok', data: {} }),
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    // LoaderPicker fires these when ServerImportView's confirm step renders.
    listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    refresh: vi.fn().mockResolvedValue(undefined),
    importInspect: vi.fn(),
    importCommit: vi.fn().mockResolvedValue({ ok: true }),
    importCancel: vi.fn().mockResolvedValue(undefined),
  },
}));

// Stub the Tauri dialog so "Choose .zip" / "Choose folder" resolve without hanging.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
}));

const mockInstance: InstanceWithStatus = {
  id: 'inst-1',
  name: 'My Instance',
  mc_version: '1.21.1',
  loader: 'fabric',
  loader_version: '0.16.0',
  max_heap_mb: 4096,
  extra_jvm_args: '',
  created_unix_ms: null,
  ready: true,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
  integrity: null,
  imported_from: null,
  created_from_server: null,
};

function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    instances: [mockInstance],
    versions: [],
    onDone: vi.fn(),
    onCancel: vi.fn(),
    ...overrides,
  };
}

describe('ServerCreateWizard', () => {
  beforeAll(() => locale.set('en'));

  it('Create button is disabled when EULA is unchecked even with a name filled', async () => {
    render(ServerCreateWizard, baseProps());

    // Fill in name
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'Test Server' } });

    // EULA is unchecked by default — Create must remain disabled
    const createBtn = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
    expect(createBtn.disabled).toBe(true);
  });

  it('Create button becomes enabled after checking EULA with name present', async () => {
    render(ServerCreateWizard, baseProps());

    // Fill in name
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'Test Server' } });

    // Check the EULA checkbox
    const eulaCheckbox = screen.getByRole('checkbox') as HTMLInputElement;
    await fireEvent.click(eulaCheckbox);

    // Create should now be enabled
    const createBtn = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
    expect(createBtn.disabled).toBe(false);
  });

  it('switching to Import mode shows the import source pickers', async () => {
    render(ServerCreateWizard, baseProps());
    await fireEvent.click(screen.getByRole('button', { name: 'Import existing' }));
    expect(screen.getByRole('button', { name: 'Choose .zip' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Choose folder' })).toBeTruthy();
  });
});
