// The instance Settings tab as a whole: its states without an instance, the
// Danger zone, provenance, the two-column split and the navigation request.
// The sections have their own suites (instance-*-section.test.ts).

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import InstanceSettingsTab from '$lib/instances/settings/InstanceSettingsTab.svelte';
import {
  clearInstanceSettingsNav,
  instanceSettingsNav,
} from '$lib/instances/settings/instance-settings-nav.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import { markSeen } from '$lib/onboarding/contextual-tours';
import { serversUi } from '$lib/servers/servers-ui.svelte';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  instanceMemoryBounds: vi.fn().mockResolvedValue({
    min_mb: 1024,
    max_mb: 8192,
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
  openImportedSourceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));
const tasks = vi.hoisted(() => ({ busyFor: null as string | null }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { ...m },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('$lib/tasks/registry.svelte', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/tasks/registry.svelte')>();
  return {
    ...actual,
    taskFor: (scope: { instanceId?: string }) =>
      tasks.busyFor !== null && scope.instanceId === tasks.busyFor ? { id: 't' } : null,
  };
});
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { list: [{ id: 'srv-1', name: 'My Server' }] },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

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

const other = makeInstance({ id: 'inst-2', name: 'Other' });

function props(over: Record<string, unknown> = {}) {
  const inst = makeInstance();
  return {
    instance: inst,
    instances: [inst, other],
    instancesLoaded: true,
    listError: null,
    fellBack: false,
    versions: [],
    running: false,
    anyRunning: false,
    onChanged: vi.fn(),
    onCreate: vi.fn(),
    onDelete: vi.fn(),
    onClone: vi.fn(),
    onTranslations: vi.fn(),
    onOpenFolder: vi.fn(),
    ...over,
  };
}

const deleteButton = () => screen.getByTestId('settings-delete-instance') as HTMLButtonElement;

describe('InstanceSettingsTab', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    tasks.busyFor = null;
    clearInstanceSettingsNav();
    serversUi.setMode('client');
    markSeen('instanceSettings');
  });

  describe('without an instance to edit', () => {
    it('waits while the list has not loaded — no "none yet", no Create', () => {
      render(InstanceSettingsTab, {
        props: props({ instance: null, instances: [], instancesLoaded: false }),
      });
      expect(screen.getByTestId('instance-settings-loading')).toBeTruthy();
      expect(screen.queryByRole('button', { name: '+ Create' })).toBeNull();
    });

    it('says the data folder is unavailable in a recovery session, with no Create', () => {
      render(InstanceSettingsTab, {
        props: props({ instance: null, instances: [], fellBack: true }),
      });
      expect(screen.getByText(/data folder, which is unavailable/i)).toBeTruthy();
      expect(screen.queryByRole('button', { name: '+ Create' })).toBeNull();
    });

    it('shows a list that could not be read as that, with no Create', () => {
      render(InstanceSettingsTab, {
        props: props({ instance: null, instances: [], listError: 'Could not read instances' }),
      });
      expect(screen.getByRole('alert').textContent).toBe('Could not read instances');
      expect(screen.queryByRole('button', { name: '+ Create' })).toBeNull();
    });

    it('offers Create only when there really are no instances', async () => {
      const p = props({ instance: null, instances: [] });
      render(InstanceSettingsTab, { props: p });
      expect(screen.getByText('No instances yet.')).toBeTruthy();
      await fireEvent.click(screen.getByRole('button', { name: '+ Create' }));
      expect(p.onCreate).toHaveBeenCalledTimes(1);
    });

    it('instances but none active (the active read failed): neutral, no Create', () => {
      render(InstanceSettingsTab, { props: props({ instance: null }) });
      expect(screen.getByText(/no instance is selected/i)).toBeTruthy();
      expect(screen.queryByRole('button', { name: '+ Create' })).toBeNull();
    });
  });

  describe('the Danger zone', () => {
    it('deletes the shown instance', async () => {
      const p = props();
      render(InstanceSettingsTab, { props: p });
      await fireEvent.click(deleteButton());
      expect(p.onDelete).toHaveBeenCalledWith('inst-1');
      expect(deleteButton().closest('[data-tour-ctx="settings-danger"]')).not.toBeNull();
    });

    it('is off with a visible reason while the game runs, a task runs, or it is the last one', () => {
      const cases: Array<[Record<string, unknown>, RegExp]> = [
        [{ running: true }, /stop the running game/i],
        [{ instances: [makeInstance()] }, /delete/i],
      ];
      for (const [over, reason] of cases) {
        const { unmount } = render(InstanceSettingsTab, { props: props(over) });
        expect(deleteButton().disabled).toBe(true);
        const id = deleteButton().getAttribute('aria-describedby');
        expect(id).toBeTruthy();
        expect(document.getElementById(id as string)?.textContent).toMatch(reason);
        unmount();
      }
      tasks.busyFor = 'inst-1';
      render(InstanceSettingsTab, { props: props() });
      expect(deleteButton().disabled).toBe(true);
      expect(
        screen.getByText('Wait for the current task on this instance to finish.'),
      ).toBeTruthy();
      expect(screen.getByTestId('settings-game-locked').textContent).toMatch(/current task/i);
    });

    it('an unread list is not a one-instance list', () => {
      render(InstanceSettingsTab, {
        props: props({ instances: [makeInstance()], listError: 'Could not read instances' }),
      });
      expect(deleteButton().disabled).toBe(false);
    });
  });

  describe('provenance and integrity', () => {
    it('shows where an imported instance came from, and opens that folder', async () => {
      render(InstanceSettingsTab, {
        props: props({
          instance: makeInstance({
            imported_from: { launcher: 'prism', source_path: 'C:/Prism/instances/x' },
          } as Partial<InstanceWithStatus>),
        }),
      });
      expect(screen.getByTestId('imported-provenance').textContent).toContain(
        'C:/Prism/instances/x',
      );
      await fireEvent.click(screen.getByTestId('open-source-folder-btn'));
      expect(m.openImportedSourceFolder).toHaveBeenCalledWith('inst-1');
    });

    it('names the server an instance was created from', () => {
      render(InstanceSettingsTab, {
        props: props({ instance: makeInstance({ created_from_server: 'srv-1' }) }),
      });
      expect(screen.getByTestId('created-from-server-provenance').textContent).toContain(
        'My Server',
      );
    });

    it('has no provenance blocks for an instance made here', () => {
      render(InstanceSettingsTab, { props: props() });
      expect(screen.queryByTestId('imported-provenance')).toBeNull();
      expect(screen.queryByTestId('created-from-server-provenance')).toBeNull();
      expect(document.querySelector('[data-tour-ctx="settings-integrity"]')).not.toBeNull();
    });
  });

  describe('a request to show a field', () => {
    it('flashes the requested field and takes the request', async () => {
      instanceSettingsNav.value = { instanceId: 'inst-1', field: 'memory' };
      const { container } = render(InstanceSettingsTab, { props: props() });
      await waitFor(() => expect(instanceSettingsNav.value).toBeNull());
      expect(container.querySelector('[data-focus-field="memory"]')?.classList).toContain(
        'field-flash',
      );
    });

    it('leaves a request about another instance for that instance', async () => {
      instanceSettingsNav.value = { instanceId: 'inst-2', field: 'name' };
      render(InstanceSettingsTab, { props: props() });
      await new Promise((r) => setTimeout(r, 0));
      expect(instanceSettingsNav.value).toEqual({ instanceId: 'inst-2', field: 'name' });
    });
  });

  it('owns its scroll container, with the gutter reserved', () => {
    render(InstanceSettingsTab, { props: props() });
    const root = screen.getByTestId('instance-settings-tab');
    expect(root.className).toContain('overflow-y-auto');
    expect(root.className).toContain('[scrollbar-gutter:stable]');
  });
});
