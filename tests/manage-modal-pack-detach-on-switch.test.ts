// Regression: switching the SELECTED instance in the Manage modal must never
// raise the pack-detach confirm. The bug: LoaderPicker was reused (not
// remounted) across instance switches, so its `prevLoader` leaked the previous
// instance's loader; the $effect then mis-read a prop-driven instance swap as a
// user-driven loader switch (resetToStable=true), flipped the saved version to
// the new ecosystem's stable, and emitted onchange — which the parent committed,
// raising the "Modpack instance / Detach & continue" prompt on a plain click.
//
// Fix: the detail-form LoaderPicker is wrapped in {#key selected.id}, so it
// remounts per instance (prevLoader resets → loaderChanged=false → the saved
// version is preserved → no onchange → no prompt).

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

// Hoisted so the spies exist before the hoisted vi.mock factory runs, while
// still being referenceable in the test bodies below.
const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  listForgeLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '47.2.0', stable: true }] }),
  listFabricLoaders: vi.fn().mockResolvedValue({
    status: 'ok',
    // Saved version 0.16.0 IS in the list but is NOT stable — so a (buggy)
    // resetToStable=true would flip it to 0.20.0 and emit onchange, whereas the
    // correct preserve path keeps 0.16.0 and emits nothing.
    data: [
      { version: '0.20.0', stable: true },
      { version: '0.16.0', stable: false },
    ],
  }),
  listQuiltLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '0.30.0', stable: true }] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  setInstanceLoader: vi.fn(),
  detachInstancePack: vi.fn(),
  checkInstanceModCompat: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { rows: [], loader_outcome: null } }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    ...m,
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceVersion: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
    createInstance: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

const { listForgeLoaders, listFabricLoaders, setInstanceLoader, detachInstancePack } = m;

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

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

function makeVersion(id = '1.20.1'): VersionEntry {
  return { id, version_type: 'release', release_date: '2024-01-01T00:00:00+00:00', url: '' };
}

const forgeInstance = makeInstance({
  id: 'inst-forge',
  name: 'Forge One',
  loader: 'forge',
  loader_version: '47.2.0',
});
const packInstance = makeInstance({
  id: 'inst-pack',
  name: 'Sodium Plus',
  loader: 'fabric',
  loader_version: '0.16.0',
  mrpack_name: 'Sodium Plus',
});

function renderModal() {
  return render(ManageInstancesModal, {
    props: {
      open: true,
      instances: [forgeInstance, packInstance],
      activeInstance: forgeInstance, // selected on open
      versions: [makeVersion()],
      onChanged: () => {},
    },
  });
}

describe('ManageInstancesModal — selecting a modpack instance does not prompt detach', () => {
  it('switching from a Forge instance to a modpack instance raises no detach prompt', async () => {
    renderModal();

    // Sanity: opens on the Forge instance.
    await waitFor(() => expect(listForgeLoaders).toHaveBeenCalled());

    // Click the modpack instance row in the sidebar list.
    const packRow = screen
      .getAllByRole('button')
      .find((b) => b.textContent?.includes('Sodium Plus') && b.querySelector('.font-medium'));
    expect(packRow).toBeTruthy();
    await fireEvent.click(packRow as HTMLElement);

    // The picker remounts for the Fabric instance and loads its versions.
    await waitFor(() => expect(listFabricLoaders).toHaveBeenCalled());

    // Give any (erroneous) async onchange a full task tick to fire and settle —
    // a macrotask drain covers the whole microtask queue in one wait, so this
    // stays correct even if the onchange→commit chain grows another await.
    await new Promise((r) => setTimeout(r, 0));

    // No detach prompt, and nothing was committed/detached by a mere switch.
    expect(screen.queryByText(/change a modpack instance/i)).toBeNull();
    expect(setInstanceLoader).not.toHaveBeenCalled();
    expect(detachInstancePack).not.toHaveBeenCalled();
  });

  it('actually changing the loader on the modpack instance still prompts detach', async () => {
    renderModal();
    await waitFor(() => expect(listForgeLoaders).toHaveBeenCalled());

    const packRow = screen
      .getAllByRole('button')
      .find((b) => b.textContent?.includes('Sodium Plus') && b.querySelector('.font-medium'));
    await fireEvent.click(packRow as HTMLElement);
    await waitFor(() => expect(listFabricLoaders).toHaveBeenCalled());

    // User explicitly switches the loader → this SHOULD prompt detach.
    const quiltBtn = screen.getByRole('button', { name: /^quilt$/i });
    await fireEvent.click(quiltBtn);

    await waitFor(() => expect(screen.queryByText(/change a modpack instance/i)).not.toBeNull());
    // Still gated behind the confirm — not committed yet.
    expect(setInstanceLoader).not.toHaveBeenCalled();
  });
});

describe('ManageInstancesModal — pack-detach dialog framing', () => {
  async function openDetachPrompt() {
    renderModal();
    await waitFor(() => expect(listForgeLoaders).toHaveBeenCalled());
    const packRow = screen
      .getAllByRole('button')
      .find((b) => b.textContent?.includes('Sodium Plus') && b.querySelector('.font-medium'));
    await fireEvent.click(packRow as HTMLElement);
    await waitFor(() => expect(listFabricLoaders).toHaveBeenCalled());
    const quiltBtn = screen.getByRole('button', { name: /^quilt$/i });
    await fireEvent.click(quiltBtn);
    await waitFor(() => expect(screen.queryByText(/change a modpack instance/i)).not.toBeNull());
  }

  it('weights the safe Keep action as primary and Detach as danger', async () => {
    await openDetachPrompt();

    const keep = screen.getByRole('button', { name: 'Keep & continue' });
    const detach = screen.getByRole('button', { name: 'Detach & continue' });
    const cancel = screen.getByRole('button', { name: 'Cancel' });

    // The safe, link-preserving path is emphasised; the irreversible one is danger.
    expect(keep.className).toContain('btn-primary');
    expect(detach.className).toContain('btn-danger');
    expect(cancel.className).toContain('btn-secondary');
  });

  it('frames the title as a question and explains both outcomes', async () => {
    await openDetachPrompt();

    // Title is a question, not a bare noun label.
    expect(screen.getByText(/change a modpack instance\?/i)).toBeTruthy();
    // Body explains the keep outcome AND the detach outcome (not just detach).
    const body = screen.getByText(/keep it linked/i);
    expect(body.textContent).toMatch(/detach/i);
  });
});
