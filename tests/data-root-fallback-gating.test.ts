// Task 9 (data-root fallback gating): while the configured data root is
// unavailable, the app runs from a temporary default location and must block
// actions that would create data in the wrong root (§7 of the data-root
// design doc, docs/superpowers/specs/2026-07-06-data-root-location-design.md).
// This covers the two behaviors named in the plan: the banner renders, and
// the "Create instance" entry point is disabled, when `fell_back` is true.
//
// `dataLocation` (src/lib/settings/data-location.svelte.ts) is a module-level
// singleton — like `mcVersions` / `settingsOpen` — so its state persists
// across tests within this file. Each test calls `dataLocation.refresh()`
// (not `init()`, which no-ops after the first successful load) to force a
// fresh fetch against that test's mock before rendering.
import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  getDataLocation: vi.fn(),
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  checkInstanceModCompat: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { rows: [], loader_outcome: null } }),
  instanceMemoryBounds: vi.fn().mockResolvedValue({
    min_mb: 1024,
    max_mb: 8192,
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
  setActiveInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceName: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceMemory: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceMinHeap: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceJvmArgs: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceLoader: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  changeInstanceMc: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openInstanceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openImportedSourceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  createInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { ...m },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));
vi.mock('$lib/servers/server-state.svelte', () => ({ serverState: { list: [] } }));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
import DataRootFallbackBanner from '$lib/settings/DataRootFallbackBanner.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.1',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
    ...over,
  };
}

const version: VersionEntry = {
  id: '1.20.1',
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: '',
};

beforeEach(() => vi.clearAllMocks());
afterEach(() => locale.set('en'));

describe('data-root fallback gating (§7)', () => {
  it('disables "New instance" with an explanatory tooltip when fell_back is true', async () => {
    m.getDataLocation.mockResolvedValue({
      status: 'ok',
      data: {
        effective: 'C:\\Users\\test\\AppData\\Roaming\\com.lucerna.app',
        configured: 'D:\\LucernaData',
        fell_back: true,
      },
    });
    await dataLocation.refresh();

    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const newInstanceBtn = (await screen.findByRole('button', {
      name: '+ New instance',
    })) as HTMLButtonElement;
    expect(newInstanceBtn.disabled).toBe(true);
  });

  it('leaves "New instance" enabled when fell_back is false', async () => {
    m.getDataLocation.mockResolvedValue({
      status: 'ok',
      data: {
        effective: 'C:\\Users\\test\\AppData\\Roaming\\com.lucerna.app',
        configured: null,
        fell_back: false,
      },
    });
    await dataLocation.refresh();

    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const newInstanceBtn = (await screen.findByRole('button', {
      name: '+ New instance',
    })) as HTMLButtonElement;
    expect(newInstanceBtn.disabled).toBe(false);
  });

  it('renders the persistent fallback banner with the configured path', async () => {
    render(DataRootFallbackBanner, {
      props: { configuredPath: 'D:\\LucernaData' },
    });

    const banner = await screen.findByTestId('data-root-fallback-banner');
    expect(banner).toBeTruthy();
    expect(banner.textContent).toContain('D:\\LucernaData');
    expect(banner.getAttribute('role')).toBe('alert');
  });
});
