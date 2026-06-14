import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, ModSource, VersionEntry } from '$lib/ipc/bindings';

// ManageInstancesModal renders LoaderPicker, which calls commands.list*Loaders
// when loader !== 'vanilla'. We use vanilla in all fixtures here so no loader
// IPC fires, but the module still needs to be mockable.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // ImportedDetailDrawer calls these at mount
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { summary: { name: 'x' }, description: '', website_url: null },
    }),
    modpackStatus: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackUpdateStatus: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'up_to_date' } }),
    // ManageInstancesModal fetches memory bounds on first open
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    // ManageInstancesModal calls these on user interaction only; mocked
    // here so imports don't throw at resolve time.
    createInstance: vi.fn(),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceVersion: vi.fn(),
    setInstanceLoader: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
    // LoaderPicker loader-version fetchers (not called for vanilla)
    listFabricLoaders: vi.fn(),
    listQuiltLoaders: vi.fn(),
    listForgeLoaders: vi.fn(),
    listNeoforgeLoaders: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
import ImportedDetailDrawer from '$lib/modpacks/ImportedDetailDrawer.svelte';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.4',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
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
    ...over,
  };
}

function makeVersion(id = '1.20.4'): VersionEntry {
  return { id, version_type: 'release', release_date: '2024-01-01T00:00:00+00:00', url: '' };
}

// Modal renders the detail panel only when instances has a match for
// activeInstance.id. Two instances so the Delete button is enabled
// (it's disabled when instances.length <= 1).
const manageProps = {
  open: true,
  instances: [makeInstance(), makeInstance({ id: 'inst-2', name: 'Second' })],
  activeInstance: makeInstance(),
  versions: [makeVersion()],
  onChanged: () => {},
};

describe('ManageInstancesModal — destructive footer pattern', () => {
  it('+ New instance is btn-primary btn-sm', () => {
    render(ManageInstancesModal, { props: manageProps });
    const btn = screen.getByRole('button', { name: /\+ new instance/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });

  it('footer Delete instance is btn-ghost-danger (LEFT) and Done is btn-primary (RIGHT)', () => {
    render(ManageInstancesModal, { props: manageProps });
    // Label is "Delete instance" (a trash <Icon> sits beside it) — match the text part
    const deleteBtn = screen.getByRole('button', { name: /delete instance/i });
    const doneBtn = screen.getByRole('button', { name: /^\s*done\s*$/i });
    expect(deleteBtn).toHaveBtnVariant('ghost-danger');
    expect(doneBtn).toHaveBtnVariant('primary');

    // The footer row uses justify-between; Done sits inside a nested flex
    // div alongside "Open folder". querySelectorAll still returns all buttons
    // in document order, so deleteBtn must precede doneBtn.
    const footer = doneBtn.closest('[class*="justify-between"]') as HTMLElement;
    expect(footer).not.toBeNull();
    const buttons = Array.from(footer.querySelectorAll<HTMLButtonElement>('button'));
    expect(buttons.indexOf(deleteBtn as HTMLButtonElement)).toBeLessThan(
      buttons.indexOf(doneBtn as HTMLButtonElement),
    );
  });
});

describe('ImportedDetailDrawer — destructive footer pattern', () => {
  it('Delete pack is btn-ghost-danger (LEFT) and Open instance is btn-primary (RIGHT)', () => {
    render(ImportedDetailDrawer, {
      props: {
        inst: makeInstance({
          id: 'pack-inst',
          mrpack_name: 'Cool Pack',
          mrpack_version: '1.0',
          mrpack_source: 'modrinth' as ModSource,
        }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const deleteBtn = screen.getByRole('button', { name: /delete pack/i });
    const openBtn = screen.getByRole('button', { name: /open instance/i });
    expect(deleteBtn).toHaveBtnVariant('ghost-danger');
    expect(openBtn).toHaveBtnVariant('primary');

    const footer = openBtn.closest('[class*="justify-between"]') as HTMLElement;
    expect(footer).not.toBeNull();
    const buttons = Array.from(footer.querySelectorAll<HTMLButtonElement>('button'));
    expect(buttons.indexOf(deleteBtn as HTMLButtonElement)).toBeLessThan(
      buttons.indexOf(openBtn as HTMLButtonElement),
    );
  });
});
