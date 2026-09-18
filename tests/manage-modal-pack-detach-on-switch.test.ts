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
import { beforeEach, describe, expect, it, vi } from 'vitest';
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
    expect(screen.queryByText(/switch the loader to/i)).toBeNull();
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

    await waitFor(() => expect(screen.queryByText(/switch the loader to/i)).not.toBeNull());
    // Still gated behind the confirm — not committed yet.
    expect(setInstanceLoader).not.toHaveBeenCalled();
  });
});

// A macrotask drain — see the first test above.
const flush = () => new Promise((r) => setTimeout(r, 0));
const pressed = (name: string) =>
  screen.getByRole('button', { name }).getAttribute('aria-pressed');
const ioError = { status: 'error', error: { kind: 'io', path: 'instance.json', details: 'denied' } };

function selectPackInstance() {
  const packRow = screen
    .getAllByRole('button')
    .find((b) => b.textContent?.includes('Sodium Plus') && b.querySelector('.font-medium'));
  return fireEvent.click(packRow as HTMLElement);
}

// Opens the prompt by switching the modpack instance (Fabric 0.16.0) to Quilt.
async function openDetachPrompt() {
  renderModal();
  await waitFor(() => expect(listForgeLoaders).toHaveBeenCalled());
  await selectPackInstance();
  await waitFor(() => expect(listFabricLoaders).toHaveBeenCalled());
  await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
  await waitFor(() => expect(screen.queryByText(/switch the loader to quilt\?/i)).not.toBeNull());
}

// The bug the maintainer reported: Cancel left the picker on the clicked loader
// although nothing had been written, and clicking the real loader to "fix" the
// highlight raised the prompt again. The same lie followed a failed write.
describe('ManageInstancesModal — a refused loader change does not stay on screen', () => {
  beforeEach(() => {
    setInstanceLoader.mockReset();
    detachInstancePack.mockReset();
  });

  it("Don't change: nothing is written and the picker goes back to the saved loader", async () => {
    await openDetachPrompt();

    await fireEvent.click(screen.getByRole('button', { name: "Don't change" }));

    await waitFor(() => expect(screen.queryByText(/switch the loader to/i)).toBeNull());
    await waitFor(() => {
      expect(pressed('Fabric')).toBe('true');
      expect(pressed('Quilt')).toBe('false');
    });
    // The pack's pinned NON-stable version is back too, not "recommended".
    await waitFor(() =>
      expect(screen.getByLabelText(/loader version/i).textContent).toContain('0.16.0'),
    );
    expect(setInstanceLoader).not.toHaveBeenCalled();
    expect(detachInstancePack).not.toHaveBeenCalled();

    // Clicking the loader the instance already has is not a change.
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    await flush();
    expect(screen.queryByText(/switch the loader to/i)).toBeNull();
    expect(setInstanceLoader).not.toHaveBeenCalled();
  });

  it('Keep the link: one loader write with the clicked ecosystem version, no detach', async () => {
    setInstanceLoader.mockResolvedValue({
      status: 'ok',
      data: { ...packInstance, loader: 'quilt', loader_version: '0.30.0' },
    });
    await openDetachPrompt();

    await fireEvent.click(screen.getByRole('button', { name: 'Keep the link' }));

    await waitFor(() => expect(setInstanceLoader).toHaveBeenCalled());
    await flush();
    expect(setInstanceLoader).toHaveBeenCalledTimes(1);
    expect(setInstanceLoader).toHaveBeenCalledWith('inst-pack', 'quilt', '0.30.0');
    expect(detachInstancePack).not.toHaveBeenCalled();
    expect(pressed('Quilt')).toBe('true');
  });

  it('a failed detach writes no loader and puts the picker back', async () => {
    detachInstancePack.mockResolvedValue(ioError);
    await openDetachPrompt();

    await fireEvent.click(screen.getByRole('button', { name: 'Detach' }));

    await waitFor(() => expect(detachInstancePack).toHaveBeenCalledWith('inst-pack'));
    await waitFor(() => {
      expect(pressed('Fabric')).toBe('true');
      expect(pressed('Quilt')).toBe('false');
    });
    expect(setInstanceLoader).not.toHaveBeenCalled();
  });

  it('a failed loader write on a plain instance puts the picker back', async () => {
    setInstanceLoader.mockResolvedValue(ioError);
    renderModal();
    await waitFor(() => expect(listForgeLoaders).toHaveBeenCalled());

    // The Forge instance is not a modpack: the write goes straight out.
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));

    await waitFor(() => expect(setInstanceLoader).toHaveBeenCalled());
    expect(setInstanceLoader).toHaveBeenCalledWith('inst-forge', 'fabric', '0.20.0');
    await waitFor(() => {
      expect(pressed('Forge')).toBe('true');
      expect(pressed('Fabric')).toBe('false');
    });
  });
});

describe('ManageInstancesModal — pack-detach dialog framing', () => {
  it('weights the safe Keep action as primary and Detach as danger', async () => {
    await openDetachPrompt();

    // EXACT names: the dialog's buttons carry the same words as the paragraphs
    // that explain them, and role-name substrings collide with sidebar text.
    const keep = screen.getByRole('button', { name: 'Keep the link' });
    const detach = screen.getByRole('button', { name: 'Detach' });
    const cancel = screen.getByRole('button', { name: "Don't change" });

    // The link-preserving path is emphasised; the irreversible one is danger.
    expect(keep.className).toContain('btn-primary');
    expect(detach.className).toContain('btn-danger');
    expect(cancel.className).toContain('btn-secondary');
  });

  it('names the change in the title and says what each choice does', async () => {
    await openDetachPrompt();

    const dialog = screen.getByRole('dialog', { name: /switch the loader to quilt\?/i });
    const text = dialog.textContent ?? '';
    // Keep: the change still happens and the pack stays updatable.
    expect(text).toMatch(/the loader changes and the pack can still be updated/i);
    // Detach: irreversible, and it does not touch the instance's files.
    expect(text).toMatch(/cannot be undone/i);
    expect(text).toMatch(/files are not touched/i);
    // The dialog describes itself to assistive tech, not just its title.
    expect(dialog.getAttribute('aria-describedby')).toBeTruthy();
  });
});
