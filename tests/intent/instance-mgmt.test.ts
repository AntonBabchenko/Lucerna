// Instance-mgmt group intent coverage: LoaderPicker variant/label classes,
// Instance-management rows not already covered by the cluster-D
// button-intents-destructive.test.ts (the Settings tab's Danger zone Delete).
//
// Rows covered here per inventory:
//   LoaderPicker:    active loader → btn-primary btn-sm
//                    inactive loader → btn-secondary btn-sm
//                    "Loader" label — uppercase text-secondary
//                    "Loader version" label — uppercase text-secondary (non-vanilla)
//   CreateInstanceDialog: Cancel → btn-secondary btn-sm, Create → btn-primary btn-sm
//   Settings tab action row: Open folder → btn-secondary btn-sm
//   DeleteInstanceDialog: Cancel → btn-secondary btn-sm

import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

// vi.mock is hoisted before imports — must appear before component imports.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
    previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
    // LoaderPicker calls these when loader !== 'vanilla'
    listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // The create dialog's memory slider fetches its bounds on mount
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    // Called on user interaction only
    createInstance: vi.fn(),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceVersion: vi.fn(),
    setInstanceLoader: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));

import CreateInstanceDialog from '$lib/instances/CreateInstanceDialog.svelte';
import DeleteInstanceDialog from '$lib/instances/DeleteInstanceDialog.svelte';
import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
import InstanceIdentitySection from '$lib/instances/settings/InstanceIdentitySection.svelte';

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.4',
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
    java: null,
    ...over,
  };
}

function makeVersion(id = '1.20.4'): VersionEntry {
  return { id, version_type: 'release', release_date: '2024-01-01T00:00:00+00:00', url: '' };
}

// ── LoaderPicker — variant per loader option ──────────────────────────────────

describe('LoaderPicker — active loader button is btn-primary btn-sm', () => {
  it('active Vanilla button has btn-primary and btn-sm', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'vanilla', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^vanilla$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });

  it('active Fabric button has btn-primary and btn-sm', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'fabric', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^fabric$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });

  it('active Forge button has btn-primary and btn-sm', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'forge', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^forge$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });
});

describe('LoaderPicker — inactive loader buttons are btn-secondary btn-sm', () => {
  it('inactive Fabric/Quilt/Forge/NeoForge buttons have btn-secondary when loader=vanilla', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'vanilla', loaderVersion: null },
    });
    for (const name of ['Fabric', 'Quilt', 'Forge', 'NeoForge']) {
      const btn = screen.getByRole('button', { name: new RegExp(`^${name}$`, 'i') });
      expect(btn).toHaveBtnVariant('secondary');
      expect(btn).toHaveBtnSize('sm');
    }
  });

  it('inactive Vanilla button has btn-secondary when loader=fabric', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'fabric', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^vanilla$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

describe('LoaderPicker — "Loader" label classes (uppercase text-secondary)', () => {
  it('"Loader" label has uppercase and text-secondary classes', () => {
    const { container } = render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'vanilla', loaderVersion: null },
    });
    // The label is a <p> with the text "Loader" (exact)
    const label = Array.from(container.querySelectorAll('p')).find(
      (el) => el.textContent?.trim() === 'Loader',
    );
    expect(label).not.toBeNull();
    const cls = label?.className ?? '';
    expect(cls).toContain('uppercase');
    expect(cls).toContain('text-secondary');
  });
});

// ── CreateInstanceDialog — Cancel + Create ───────────────────────────────────

const createProps = {
  versions: [makeVersion()],
  activate: async () => null,
  onCreated: () => {},
  onClose: () => {},
};

describe('CreateInstanceDialog — Cancel is btn-secondary btn-sm', () => {
  it('Cancel has btn-secondary and btn-sm', async () => {
    render(CreateInstanceDialog, { props: createProps });
    const cancelBtn = await screen.findByRole('button', { name: /^cancel$/i });
    expect(cancelBtn).toHaveBtnVariant('secondary');
    expect(cancelBtn).toHaveBtnSize('sm');
  });
});

describe('CreateInstanceDialog — Create is btn-primary btn-sm', () => {
  it('Create button has btn-primary and btn-sm', async () => {
    render(CreateInstanceDialog, { props: createProps });
    const createBtn = await screen.findByTestId('create-instance-submit');
    expect(createBtn).toHaveBtnVariant('primary');
    expect(createBtn).toHaveBtnSize('sm');
  });
});

// ── Settings tab — the action row ───────────────────────────────────────────

describe('Settings tab action row: Open folder is btn-secondary btn-sm', () => {
  it('"Open folder" button has btn-secondary and btn-sm', () => {
    render(InstanceIdentitySection, {
      props: {
        instance: makeInstance(),
        running: false,
        busy: false,
        onChanged: () => {},
        onClone: () => {},
        onTranslations: () => {},
        onOpenFolder: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /open folder/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

// ── DeleteInstanceDialog — Cancel ────────────────────────────────────────────

describe('DeleteInstanceDialog — Cancel is btn-secondary btn-sm', () => {
  it('Cancel in the delete confirm has btn-secondary and btn-sm', async () => {
    render(DeleteInstanceDialog, {
      props: {
        instance: makeInstance(),
        refresh: async () => {},
        onDeleted: () => {},
        onClose: () => {},
      },
    });
    const cancelBtn = await screen.findByRole('button', { name: /^cancel$/i });
    expect(cancelBtn).toHaveBtnVariant('secondary');
    expect(cancelBtn).toHaveBtnSize('sm');
  });
});
