// The Game section of the instance Settings tab: Minecraft version, loader,
// the compatibility summary and the pack-detach question. Moved here with its
// tests from the Manage window (manage-modal-pack-detach-on-switch,
// manage-loader-change-serialization, the compat half of
// manage-modal-error-surfacing, the MC half of manage-focus-highlight). Where
// those tests clicked another row of the window's list, these re-render the
// section with another instance: the tab always shows the active one, and an
// outside switch is a new `instance` prop.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { __resetCfgQueueForTest } from '$lib/instances/settings/cfg-queue.svelte';
import InstanceGameSection from '$lib/instances/settings/InstanceGameSection.svelte';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';
import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import * as toasts from '$lib/toasts/toasts.svelte';

const m = vi.hoisted(() => ({
  listForgeLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '47.2.0', stable: true }] }),
  listFabricLoaders: vi.fn().mockResolvedValue({
    status: 'ok',
    // Saved version 0.16.0 IS in the list but is NOT stable — so a (buggy)
    // reset to stable would flip it to 0.20.0 and emit onchange, whereas the
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
  changeInstanceMc: vi.fn(),
  detachInstancePack: vi.fn(),
  scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  checkInstanceModCompat: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { rows: [], loader_outcome: null } }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { ...m },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
// The migration dialog is its own suite; here only the props the section hands
// it matter — which instance it is for, and what its Apply does here.
type MigrationProps = { instanceId: string; onApplied?: () => void; onClose: () => void };
const migration = vi.hoisted(() => ({ props: null as MigrationProps | null }));
vi.mock('$lib/mods/MigrationPlanDialog.svelte', () => ({
  default: (_anchor: unknown, props: MigrationProps) => {
    migration.props = props;
    return {};
  },
}));
// Keep the real toast module's other exports, spy only the push helpers.
vi.mock('$lib/toasts/toasts.svelte', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/toasts/toasts.svelte')>();
  return { ...actual, pushSuccess: vi.fn(), pushWarning: vi.fn(), pushInfo: vi.fn() };
});

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
    java: null,
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

function props(over: Record<string, unknown> = {}) {
  return {
    instance: forgeInstance,
    versions: [makeVersion()],
    running: false,
    busy: false,
    onChanged: vi.fn(),
    ...over,
  };
}

// A macrotask drain covers the whole microtask queue in one wait.
const flush = () => new Promise((r) => setTimeout(r, 0));
const pressed = (name: string) => screen.getByRole('button', { name }).getAttribute('aria-pressed');
const ioError = {
  status: 'error',
  error: { kind: 'io', path: 'instance.json', details: 'denied' },
};

beforeEach(() => {
  vi.clearAllMocks();
  m.setInstanceLoader.mockReset();
  m.detachInstancePack.mockReset();
  m.changeInstanceMc.mockReset();
  __resetCfgQueueForTest();
  invalidateCompatScan();
});

describe('switching to a modpack instance does not prompt detach', () => {
  it('a new instance shown (an outside switch) raises no detach prompt', async () => {
    const { rerender } = render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());

    await rerender(props({ instance: packInstance }));
    await waitFor(() => expect(m.listFabricLoaders).toHaveBeenCalled());
    await flush();

    expect(screen.queryByText(/switch the loader to/i)).toBeNull();
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
    expect(m.detachInstancePack).not.toHaveBeenCalled();
  });

  it('actually changing the loader on the modpack instance still prompts detach', async () => {
    render(InstanceGameSection, { props: props({ instance: packInstance }) });
    await waitFor(() => expect(m.listFabricLoaders).toHaveBeenCalled());

    await fireEvent.click(screen.getByRole('button', { name: /^quilt$/i }));

    await waitFor(() => expect(screen.queryByText(/switch the loader to/i)).not.toBeNull());
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
  });
});

async function openDetachPrompt() {
  render(InstanceGameSection, { props: props({ instance: packInstance }) });
  await waitFor(() => expect(m.listFabricLoaders).toHaveBeenCalled());
  await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
  await waitFor(() => expect(screen.queryByText(/switch the loader to quilt\?/i)).not.toBeNull());
}

// The bug the maintainer reported: Cancel left the picker on the clicked loader
// although nothing had been written. The same lie followed a failed write.
describe('a refused loader change does not stay on screen', () => {
  it("Don't change: nothing is written and the picker goes back to the saved loader", async () => {
    await openDetachPrompt();
    await fireEvent.click(screen.getByRole('button', { name: "Don't change" }));

    await waitFor(() => expect(screen.queryByText(/switch the loader to/i)).toBeNull());
    await waitFor(() => {
      expect(pressed('Fabric')).toBe('true');
      expect(pressed('Quilt')).toBe('false');
    });
    await waitFor(() =>
      expect(screen.getByLabelText(/loader version/i).textContent).toContain('0.16.0'),
    );
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
    expect(m.detachInstancePack).not.toHaveBeenCalled();

    // Clicking the loader the instance already has is not a change.
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    await flush();
    expect(screen.queryByText(/switch the loader to/i)).toBeNull();
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
  });

  it('Keep the link: one loader write with the clicked ecosystem version, no detach', async () => {
    m.setInstanceLoader.mockResolvedValue({
      status: 'ok',
      data: { ...packInstance, loader: 'quilt', loader_version: '0.30.0' },
    });
    await openDetachPrompt();
    await fireEvent.click(screen.getByRole('button', { name: 'Keep the link' }));

    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalled());
    await flush();
    expect(m.setInstanceLoader).toHaveBeenCalledTimes(1);
    expect(m.setInstanceLoader).toHaveBeenCalledWith('inst-pack', 'quilt', '0.30.0');
    expect(m.detachInstancePack).not.toHaveBeenCalled();
  });

  it('a failed detach writes no loader and puts the picker back', async () => {
    m.detachInstancePack.mockResolvedValue(ioError);
    await openDetachPrompt();
    await fireEvent.click(screen.getByRole('button', { name: 'Detach' }));

    await waitFor(() => expect(m.detachInstancePack).toHaveBeenCalledWith('inst-pack'));
    await waitFor(() => {
      expect(pressed('Fabric')).toBe('true');
      expect(pressed('Quilt')).toBe('false');
    });
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
  });

  it('a failed loader write on a plain instance puts the picker back', async () => {
    m.setInstanceLoader.mockResolvedValue(ioError);
    render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());

    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));

    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalled());
    expect(m.setInstanceLoader).toHaveBeenCalledWith('inst-forge', 'fabric', '0.20.0');
    await waitFor(() => {
      expect(pressed('Forge')).toBe('true');
      expect(pressed('Fabric')).toBe('false');
    });
  });
});

// A stale saved loader version used to raise the keep/detach question with
// nobody having clicked anything — an irreversible choice over a version string.
describe('a modpack instance changes only on the user say-so', () => {
  it('a stale saved loader version raises no prompt and writes nothing', async () => {
    const stalePack = makeInstance({
      id: 'inst-stale-pack',
      name: 'Stale Pack',
      loader: 'fabric',
      loader_version: '0.1.0-gone',
      mrpack_name: 'Stale Pack',
    });
    render(InstanceGameSection, { props: props({ instance: stalePack }) });
    await waitFor(() => expect(m.listFabricLoaders).toHaveBeenCalled());
    await flush();

    expect(screen.queryByRole('dialog', { name: /switch/i })).toBeNull();
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
    expect(m.detachInstancePack).not.toHaveBeenCalled();
    expect(screen.getByLabelText(/loader version/i).textContent).not.toContain('0.20.0');
  });

  it('picking another version of the same loader asks about the VERSION, not the loader', async () => {
    render(InstanceGameSection, { props: props({ instance: packInstance }) });
    const trigger = await screen.findByLabelText(/loader version/i);
    await waitFor(() => expect(trigger.textContent).toContain('0.16.0'));

    await fireEvent.click(trigger);
    await fireEvent.mouseDown(screen.getByRole('option', { name: /0\.20\.0/ }));

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: /switch the loader version to 0\.20\.0\?/i }),
      ).not.toBeNull(),
    );
    expect(screen.queryByText(/switch the loader to/i)).toBeNull();
  });
});

describe('the pack-detach dialog', () => {
  it('weights the safe Keep action as primary and Detach as danger', async () => {
    await openDetachPrompt();
    expect(screen.getByRole('button', { name: 'Keep the link' }).className).toContain(
      'btn-primary',
    );
    expect(screen.getByRole('button', { name: 'Detach' }).className).toContain('btn-danger');
    expect(screen.getByRole('button', { name: "Don't change" }).className).toContain(
      'btn-secondary',
    );
  });

  it('names the change in the title and says what each choice does', async () => {
    await openDetachPrompt();
    const dialog = screen.getByRole('dialog', { name: /switch the loader to quilt\?/i });
    const text = dialog.textContent ?? '';
    expect(text).toMatch(/the pack can still be updated/i);
    expect(text).toMatch(/cannot be undone/i);
    expect(text).toMatch(/files are not touched/i);
    expect(dialog.getAttribute('aria-describedby')).toBeTruthy();
  });

  it('an outside switch of the active instance answers the prompt with no', async () => {
    const { rerender } = render(InstanceGameSection, {
      props: props({ instance: packInstance }),
    });
    await waitFor(() => expect(m.listFabricLoaders).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(screen.queryByText(/switch the loader to quilt\?/i)).not.toBeNull());

    await rerender(props({ instance: forgeInstance }));
    await waitFor(() => expect(screen.queryByText(/switch the loader to/i)).toBeNull());
    expect(m.setInstanceLoader).not.toHaveBeenCalled();
  });
});

const okLoader = (loader: string) => ({
  status: 'ok',
  data: makeInstance({ ...forgeInstance, loader: loader as InstanceWithStatus['loader'] }),
});

describe('loader changes are serialized (last click wins)', () => {
  it('overtaken commands are silenced, queued older ones discarded', async () => {
    const release: Array<(v: unknown) => void> = [];
    m.setInstanceLoader.mockImplementation(() => new Promise((res) => release.push(res)));
    const onChanged = vi.fn();
    render(InstanceGameSection, { props: props({ onChanged }) });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());

    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(1));
    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
    await flush();
    expect(m.setInstanceLoader).toHaveBeenCalledTimes(1);

    release[0](okLoader('fabric'));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(2));
    expect(m.setInstanceLoader.mock.calls[1][1]).toBe('quilt');
    expect(onChanged).not.toHaveBeenCalled();
    expect(m.scanInstanceModCompat).not.toHaveBeenCalled();

    release[1](okLoader('quilt'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    await flush();
    await flush();
    expect(m.setInstanceLoader).toHaveBeenCalledTimes(2);
    expect(m.scanInstanceModCompat).toHaveBeenCalledTimes(1);
  });

  // The section unmounts on every tab switch. A queue that lived in the
  // component would start empty on the way back, and a click then could run
  // beside the one still in flight — whichever landed last would win.
  it('a remounted section joins the change still in flight', async () => {
    const release: Array<(v: unknown) => void> = [];
    m.setInstanceLoader.mockImplementation(() => new Promise((res) => release.push(res)));
    const first = render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(1));

    first.unmount();
    render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Quilt' })).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
    await flush();
    // Queued behind Fabric, not sent beside it.
    expect(m.setInstanceLoader).toHaveBeenCalledTimes(1);

    release[0](okLoader('fabric'));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(2));
    expect(m.setInstanceLoader.mock.calls[1][1]).toBe('quilt');
    release[1](okLoader('quilt'));
  });

  // After a remount the pickers show the SAVED config, not the change about to
  // land: the section says a change is still being applied, until it lands.
  // A change started in this mount needs no line — the picker shows the click.
  // The version field is the saved value until the change lands (unlike the
  // loader picker, which shows the click), so a version change in this mount
  // says it is being applied too.
  it('a version change says it is being applied until it lands', async () => {
    let resolve!: (v: unknown) => void;
    m.changeInstanceMc.mockReturnValue(new Promise((r) => (resolve = r)));
    render(InstanceGameSection, {
      props: props({ versions: [makeVersion(), makeVersion('1.21.1')] }),
    });
    await pickMc('1.21.1');
    expect(await screen.findByTestId('settings-game-applying')).toBeTruthy();
    resolve({
      status: 'ok',
      data: {
        instance: makeInstance({ ...forgeInstance, mc_version: '1.21.1' }),
        loader_outcome: { kind: 'unchanged' },
      },
    });
    await waitFor(() => expect(screen.queryByTestId('settings-game-applying')).toBeNull());
  });

  it('a remounted section says a change is still being applied, until it lands', async () => {
    const release: Array<(v: unknown) => void> = [];
    m.setInstanceLoader.mockImplementation(() => new Promise((res) => release.push(res)));
    const first = render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId('settings-game-applying')).toBeNull();

    first.unmount();
    render(InstanceGameSection, { props: props() });
    expect((await screen.findByTestId('settings-game-applying')).textContent).toContain(
      'still being applied',
    );
    // Not a lock: last click still wins (the test above).
    expect((screen.getByRole('button', { name: 'Quilt' }) as HTMLButtonElement).disabled).toBe(
      false,
    );

    release[0](okLoader('fabric'));
    await waitFor(() => expect(screen.queryByTestId('settings-game-applying')).toBeNull());
  });
});

describe('an IPC call that throws', () => {
  it('a version change says why, instead of vanishing into the queue', async () => {
    m.changeInstanceMc.mockRejectedValue(new Error('ipc channel closed'));
    render(InstanceGameSection, {
      props: props({ versions: [makeVersion(), makeVersion('1.21.1')] }),
    });
    await pickMc('1.21.1');
    expect(await screen.findByText(/ipc channel closed/)).toBeTruthy();
  });

  it('a loader change says why, and the picker goes back', async () => {
    m.setInstanceLoader.mockRejectedValue(new Error('ipc channel closed'));
    render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    expect(await screen.findByText(/ipc channel closed/)).toBeTruthy();
    await waitFor(() => expect(pressed('Forge')).toBe('true'));
  });
});

async function pickMc(id: string) {
  await fireEvent.click(screen.getByRole('combobox', { name: 'Minecraft version' }));
  await fireEvent.mouseDown(await screen.findByRole('option', { name: id }));
}

describe('a change that finishes after the tab is gone', () => {
  it('still refreshes and says its outcome, but rescans nothing', async () => {
    let resolve!: (v: unknown) => void;
    m.changeInstanceMc.mockReturnValue(new Promise((r) => (resolve = r)));
    const onChanged = vi.fn();
    const { unmount } = render(InstanceGameSection, {
      props: props({ versions: [makeVersion(), makeVersion('1.21.1')], onChanged }),
    });
    await pickMc('1.21.1');
    await waitFor(() => expect(m.changeInstanceMc).toHaveBeenCalledWith('inst-forge', '1.21.1'));

    unmount(); // another tab was clicked
    resolve({
      status: 'ok',
      data: {
        instance: makeInstance({ ...forgeInstance, mc_version: '1.21.1', loader: 'vanilla' }),
        loader_outcome: { kind: 'loader_reset_to_vanilla', previous_loader: 'forge' },
      },
    });
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    await flush();
    expect(vi.mocked(toasts.pushWarning)).toHaveBeenCalled();
    expect(m.scanInstanceModCompat).not.toHaveBeenCalled();
  });

  it('a failure is a toast naming the instance and the field', async () => {
    let resolve!: (v: unknown) => void;
    m.changeInstanceMc.mockReturnValue(new Promise((r) => (resolve = r)));
    const { unmount } = render(InstanceGameSection, {
      props: props({ versions: [makeVersion(), makeVersion('1.21.1')] }),
    });
    await pickMc('1.21.1');
    await waitFor(() => expect(m.changeInstanceMc).toHaveBeenCalled());
    unmount();
    resolve(ioError);
    await waitFor(() => expect(vi.mocked(toasts.pushWarning)).toHaveBeenCalled());
    expect(vi.mocked(toasts.pushWarning).mock.calls[0][0]).toBe(
      "Couldn't save Minecraft version for Forge One",
    );
  });
});

describe('the Game section', () => {
  it('says the version list is unavailable when there are no versions at all', () => {
    render(InstanceGameSection, { props: props({ versions: [] }) });
    expect(screen.getByText(/version list unavailable/i)).toBeTruthy();
  });

  it('nudges to show snapshots when only snapshot versions exist', () => {
    const snapshot: VersionEntry = {
      id: '24w01a',
      version_type: 'snapshot',
      release_date: '2024-01-01T00:00:00+00:00',
      url: '',
    };
    render(InstanceGameSection, { props: props({ versions: [snapshot] }) });
    expect(screen.getByText(/no release versions available/i)).toBeTruthy();
  });

  it("clears a refused change's message once the next change works", async () => {
    m.changeInstanceMc
      .mockResolvedValueOnce({ status: 'error', error: { kind: 'instance_busy' } })
      .mockResolvedValueOnce({
        status: 'ok',
        data: {
          instance: makeInstance({ mc_version: '1.21.1' }),
          loader_outcome: { kind: 'unchanged' },
        },
      });
    render(InstanceGameSection, {
      props: props({
        instance: makeInstance(),
        versions: [makeVersion(), makeVersion('1.21'), makeVersion('1.21.1')],
      }),
    });
    await pickMc('1.21');
    expect(await screen.findByText(/already in progress/i)).toBeTruthy();
    await pickMc('1.21.1');
    await waitFor(() => expect(m.changeInstanceMc).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText(/already in progress/i)).toBeNull());
  });

  it('picking the empty row writes nothing — a version is never cleared from here', async () => {
    render(InstanceGameSection, { props: props({ instance: makeInstance() }) });
    await fireEvent.click(screen.getByRole('combobox', { name: 'Minecraft version' }));
    await fireEvent.mouseDown(screen.getByRole('option', { name: /choose/i }));
    await flush();
    expect(m.changeInstanceMc).not.toHaveBeenCalled();
  });

  it('says in plain words why the version and loader are locked', () => {
    const { unmount } = render(InstanceGameSection, { props: props({ running: true }) });
    expect(screen.getByTestId('settings-game-locked').textContent).toContain('Stop the game');
    unmount();
    render(InstanceGameSection, { props: props({ busy: true }) });
    expect(screen.getByTestId('settings-game-locked').textContent).toContain('current task');
  });

  it('shows the compat summary in a polite (role=status) region after a loader change', async () => {
    m.setInstanceLoader.mockResolvedValue(okLoader('quilt'));
    m.scanInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [
        {
          sha1: 'a',
          loader_mismatch: false,
          detected_loader: null,
          live_checkable: true,
          platform_mismatch: true,
          platform_axis: 'minecraft',
          platform_declared: '[1.20,)',
        },
      ],
    });
    render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
    const summary = await screen.findByText(/no version for this configuration/i);
    expect(summary.closest('[role="status"]')).not.toBeNull();
    expect(summary.closest('[role="alert"]')).toBeNull();
  });

  describe('the migration plan', () => {
    const mismatch = {
      status: 'ok',
      data: [
        {
          sha1: 'a',
          loader_mismatch: false,
          detected_loader: null,
          live_checkable: true,
          platform_mismatch: true,
          platform_axis: 'minecraft',
          platform_declared: '[1.20,)',
        },
      ],
    };

    async function openPlan() {
      m.setInstanceLoader.mockResolvedValue(okLoader('quilt'));
      m.scanInstanceModCompat.mockResolvedValue(mismatch);
      const view = render(InstanceGameSection, { props: props() });
      await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());
      await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
      await fireEvent.click(await screen.findByTestId('open-migration-plan-btn'));
      await waitFor(() => expect(migration.props).not.toBeNull());
      return view;
    }

    beforeEach(() => {
      migration.props = null;
    });

    it('is opened for the instance shown, and its Apply rescans that instance', async () => {
      await openPlan();
      expect(migration.props?.instanceId).toBe('inst-forge');
      const before = m.scanInstanceModCompat.mock.calls.length;
      migration.props?.onApplied?.();
      await waitFor(() => expect(m.scanInstanceModCompat.mock.calls.length).toBe(before + 1));
      expect(m.scanInstanceModCompat.mock.calls.at(-1)?.[0]).toBe('inst-forge');
    });

    it('an Apply that lands once another instance is shown rescans nothing', async () => {
      // The compat store is shared: a scan for an instance no longer shown
      // would land under the shown one's Overview.
      const { rerender } = await openPlan();
      const applied = migration.props?.onApplied;
      await rerender(props({ instance: makeInstance({ id: 'inst-other', name: 'Other' }) }));
      const before = m.scanInstanceModCompat.mock.calls.length;
      applied?.();
      await flush();
      expect(m.scanInstanceModCompat.mock.calls.length).toBe(before);
    });
  });

  it('shows a quiet "couldn\'t check" note (role=status) when the compat scan throws', async () => {
    m.setInstanceLoader.mockResolvedValue(okLoader('quilt'));
    m.scanInstanceModCompat.mockRejectedValue(new Error('transport failure'));
    render(InstanceGameSection, { props: props() });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
    const note = await screen.findByText(/couldn't check mod compatibility/i);
    expect(note.closest('[role="status"]')).not.toBeNull();
    m.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
  });

  it('flashes AND focuses the Minecraft version field when asked to', async () => {
    const onFocusDelivered = vi.fn();
    const { container } = render(InstanceGameSection, {
      props: props({ focusField: 'mc', onFocusDelivered }),
    });
    expect(container.querySelector('[data-focus-field="mc"]')?.classList).toContain('field-flash');
    await waitFor(() => expect(onFocusDelivered).toHaveBeenCalled());
  });
});
