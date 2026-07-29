import { render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { listInstalled, modpackStatus, modsProjects, getVersions, updateStatus } = vi.hoisted(
  () => ({
    listInstalled: vi.fn(),
    modpackStatus: vi.fn(),
    modsProjects: vi.fn(),
    getVersions: vi.fn(),
    updateStatus: vi.fn(),
  }),
);

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: listInstalled,
    modpackStatus,
    modsProjects,
    modpackGetVersions: getVersions,
    modpackUpdateStatus: updateStatus,
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ImportedDetailDrawer from '$lib/modpacks/ImportedDetailDrawer.svelte';

const inst = {
  id: 'inst-1',
  mrpack_name: 'RLCraft',
  mrpack_version: '1.0.0',
  mrpack_version_id: 'v3',
  mrpack_project_id: 'proj',
  mrpack_source: 'modrinth',
} as never;

const props = () => ({
  inst,
  onClose: vi.fn(),
  onOpenInstance: vi.fn(),
  onDeleted: vi.fn(),
  onUpdated: vi.fn(),
});

beforeEach(() => {
  vi.clearAllMocks();
  listInstalled.mockResolvedValue({ status: 'ok', data: [] });
  modsProjects.mockResolvedValue({ status: 'ok', data: [] });
  getVersions.mockResolvedValue({ status: 'ok', data: [] });
  // Up to date — the switch affordance must still be offered.
  updateStatus.mockResolvedValue({ status: 'ok', data: { kind: 'up_to_date' } });
  modpackStatus.mockResolvedValue({
    status: 'ok',
    data: {
      origin: {
        project_id: 'proj',
        source: 'modrinth',
        project_name: 'RLCraft',
        version: '1.0.0',
        files: [],
        missing_mods: [],
        skipped_overrides: [],
        resolved_missing: [],
        inert_loader_jars: [],
      },
      installed_shas: [],
      removed_files: [],
      added_count: 0,
      is_modified: false,
      missing_mods: [],
    },
  });
});

describe('Imported drawer — change version entry point', () => {
  it('offers "Change version" even when the pack is up to date', async () => {
    // Reaching an OLDER version while already up to date is the whole point, so
    // the affordance must not be gated on an available update.
    render(ImportedDetailDrawer, props());
    await waitFor(() => expect(screen.getByTestId('imported-detail-switch-version')).toBeTruthy());
  });

  it('hides "Change version" for a pack with no recorded project id', async () => {
    // A drag-drop import with no provenance has no version list to offer.
    render(ImportedDetailDrawer, {
      ...props(),
      inst: { ...(inst as object), mrpack_project_id: null } as never,
    });
    await waitFor(() => expect(listInstalled).toHaveBeenCalled());
    expect(screen.queryByTestId('imported-detail-switch-version')).toBeNull();
  });
});
