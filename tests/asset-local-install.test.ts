import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { commands } from '$lib/ipc/bindings';
import AddonsTab from '$lib/mods/AddonsTab.svelte';

// AddonsTab mounts ModBrowseView (fires IPC on mount) + the lazy installed
// views. Mirror tests/addons-tab.test.ts's full bindings mock so nothing trips
// on a missing command/event, and add the new asset-install command.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        loader_mismatch: false,
        mc_mismatch: false,
        detected_loader: null,
        detected_mc: null,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetInstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
const openMock = vi.fn().mockResolvedValue(['/x/Faithful.zip', '/x/Bad.zip']);
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => openMock(...a) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

const toasts = vi.hoisted(() => ({ success: vi.fn(), warning: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => toasts.success(...a),
  pushWarning: (...a: unknown[]) => toasts.warning(...a),
}));

afterEach(async () => {
  vi.clearAllMocks();
  const s = await import('$lib/settings/state.svelte');
  s.droppedAssets.value = null;
  s.addonsKind.value = 'mod';
  s.assetsChanged.value = 0;
});

describe('AddonsTab manual asset install', () => {
  it('clicking the resource-pack dropzone installs picked zips and bumps assetsChanged', async () => {
    vi.mocked(commands.assetInstallLocal)
      .mockResolvedValueOnce({ status: 'ok', data: null } as never)
      .mockResolvedValueOnce({ status: 'error', error: 'boom' } as never);
    render(AddonsTab, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Resource packs' }));
    await fireEvent.click(screen.getByTestId('file-dropzone'));
    await waitFor(() => {
      expect(vi.mocked(commands.assetInstallLocal)).toHaveBeenCalledWith(
        'i',
        'resource_pack',
        '/x/Faithful.zip',
      );
      expect(vi.mocked(commands.assetInstallLocal)).toHaveBeenCalledWith(
        'i',
        'resource_pack',
        '/x/Bad.zip',
      );
    });
    expect(toasts.success).toHaveBeenCalled();
    expect(toasts.warning).toHaveBeenCalled();
    const { assetsChanged } = await import('$lib/settings/state.svelte');
    expect(assetsChanged.value).toBeGreaterThan(0);
  });

  it('consumes a droppedAssets payload matching the active kind', async () => {
    render(AddonsTab, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    const s = await import('$lib/settings/state.svelte');
    s.droppedAssets.value = { kind: 'shader', paths: ['/x/BSL.zip'] };
    await waitFor(() => {
      expect(vi.mocked(commands.assetInstallLocal)).toHaveBeenCalledWith(
        'i',
        'shader',
        '/x/BSL.zip',
      );
    });
    expect(s.droppedAssets.value).toBeNull();
  });

  it('the dropzone is disabled when no instance is selected', async () => {
    render(AddonsTab, { props: { instanceId: null, mcVersion: null, loader: null } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Resource packs' }));
    const zone = screen.getByTestId('file-dropzone');
    expect(zone.getAttribute('aria-disabled')).toBe('true');
  });
});
