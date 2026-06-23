// AddonsTab local-jar install busy state. Driving the real inspect→install
// flow: the dropzone click opens the file picker (mocked), modsInspectLocal
// returns a mismatch so the CompatWarningDialog renders with its "Install
// anyway" button. Clicking it runs modsInstallLocal — which we make a
// never-resolving deferred so we can observe the button's busy state
// mid-flight (Spinner role="status" + disabled), then clears on resolve.
//
// As with tests/addons-tab.test.ts, AddonsTab embeds ModBrowseView /
// InstalledModsView / InstalledAssetsView which all fire IPC on mount, so we
// mock the full bindings layer.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// A deferred that modsInstallLocal returns — stays pending until we resolve it,
// letting us assert the busy state while the install is in flight.
let resolveInstall: (v: { status: 'ok'; data: null }) => void;
const installDeferred = () =>
  new Promise<{ status: 'ok'; data: null }>((resolve) => {
    resolveInstall = resolve;
  });

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // Return a mismatch so onJarsPicked routes through the CompatWarningDialog,
    // which is the surface that carries the install button.
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        loader_mismatch: true,
        mc_mismatch: false,
        detected_loader: 'forge',
        detected_mc: null,
      },
    }),
    modsInstallLocal: vi.fn().mockImplementation(() => installDeferred()),
    assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetInstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

// The dropzone click opens the file picker — return a single jar path.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(['/tmp/example-mod.jar']),
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import AddonsTab from '$lib/mods/AddonsTab.svelte';
import { markSeen } from '$lib/onboarding/contextual-tours';

beforeEach(() => markSeen('addons'));

const props = {
  instanceId: 'i',
  instanceName: 'Test',
  mcVersion: '1.20.1',
  loader: 'fabric' as const,
};

describe('AddonsTab — local install busy state', () => {
  afterEach(async () => {
    const { droppedMods, modBrowseOpenProject } = await import('$lib/settings/state.svelte');
    droppedMods.value = null;
    modBrowseOpenProject.value = null;
    vi.clearAllMocks();
  });

  it('shows a busy spinner on the install button while the install is in flight, and clears on resolve', async () => {
    render(AddonsTab, { props });

    // Trigger the file pick via the dropzone affordance. openFile returns a jar
    // path, modsInspectLocal flags it as a loader mismatch, so the compat
    // warning dialog renders with its "Install anyway" button.
    await fireEvent.click(screen.getByTestId('file-dropzone'));

    const installBtn = await screen.findByRole('button', { name: /install anyway/i });

    // Not busy before the click — no spinner yet.
    expect(installBtn.querySelector('[role="status"]')).toBeNull();

    // Click install — modsInstallLocal is pending, so the button goes busy.
    await fireEvent.click(installBtn);

    await waitFor(() => {
      const busyBtn = screen.getByRole('button', { name: /install anyway/i });
      expect(busyBtn.querySelector('[role="status"]')).not.toBeNull();
      expect((busyBtn as HTMLButtonElement).disabled).toBe(true);
    });

    // Resolve the install — the dialog closes (install done) and the busy state
    // is cleared. The "Install anyway" button is gone.
    resolveInstall({ status: 'ok', data: null });

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: /install anyway/i })).toBeNull();
    });
  });
});
