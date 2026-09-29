/**
 * «Install {dep}» on a missing dependency says what really happened. A dependency the profile
 * already lists is no miss: the toast says it is installed and the pre-flight re-checks — a search
 * would invite the second copy that stops the game. A busy profile is no miss either (a search
 * could not install it now). Only a real miss, or a failure a manual pick may get past, opens the
 * search — and a failure says why.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { formatError } from '$lib/ipc/format-error';

const h = vi.hoisted(() => ({
  waystones: {
    filename: 'waystones.jar',
    sha1: 'w',
    source: 'modrinth',
    project_id: 'PW',
    version_id: 'v',
    name: 'Waystones',
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  },
  instanceDependencyPreflight: vi.fn(),
  modsInstallMissingRequired: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushInfo: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [h.waystones] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: h.instanceDependencyPreflight,
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsInstallMissingRequired: h.modsInstallMissingRequired,
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: h.pushSuccess,
  pushWarning: h.pushWarning,
  pushActionToast: vi.fn(),
  pushInfo: h.pushInfo,
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

const missingBalm = {
  dependent_sha1: 'w',
  dependent_name: 'Waystones',
  dep_id: 'balm',
  kind: 'missing_required',
  installed_version: null,
  needed: '',
  needed_desc: { raw: '', family: 'maven', alternatives: [], unparseable: false, soft: false },
  provider_project: null,
  provider_sha1: null,
  family: null,
};

// A DISTINCT instance id per case: the pre-flight cache is a per-instance LRU.
const view = (instanceId: string, onBrowseFor = vi.fn()) => {
  render(InstalledModsView, {
    props: { instanceId, mcVersion: '1.21.1', loader: 'neoforge' as const, onBrowseFor },
  });
  return onBrowseFor;
};
const installFromPanel = async () =>
  fireEvent.click(
    within(await screen.findByTestId('preflight-panel')).getByRole('button', {
      name: 'Install balm',
    }),
  );

beforeEach(() => {
  h.instanceDependencyPreflight.mockReset();
  h.instanceDependencyPreflight.mockResolvedValue({
    status: 'ok',
    data: { violations: [missingBalm] },
  });
  h.modsInstallMissingRequired.mockReset();
  h.pushSuccess.mockReset();
  h.pushWarning.mockReset();
  h.pushInfo.mockReset();
});

describe('«Install» on a missing dependency', () => {
  it('a dependency the profile already lists says so and re-checks — never a search', async () => {
    const error = { kind: 'mods_already_installed', name: 'Balm' };
    h.modsInstallMissingRequired.mockResolvedValue({ status: 'error', error });
    const onBrowseFor = view('missing-already');
    await installFromPanel();

    await waitFor(() => expect(h.pushInfo).toHaveBeenCalledWith('Balm is already installed'));
    // The report is read again: whatever it says now is the truth about that dependency.
    await waitFor(() => expect(h.instanceDependencyPreflight).toHaveBeenCalledTimes(2));
    expect(onBrowseFor).not.toHaveBeenCalled();
    expect(h.pushWarning).not.toHaveBeenCalled();
  });

  it('a busy profile says so — a search could not install it now either', async () => {
    h.modsInstallMissingRequired.mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_busy' },
    });
    const onBrowseFor = view('missing-busy');
    await installFromPanel();

    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith("Couldn't install balm", [
        'Another operation — such as a modpack update, a migration or a clone — is using this profile. Try again once it finishes.',
      ]),
    );
    expect(onBrowseFor).not.toHaveBeenCalled();
  });

  it('a failure a manual pick may get past still opens the search — and says why', async () => {
    const error = { kind: 'mods_network', url: 'https://cdn.modrinth.com', details: 'reset' };
    h.modsInstallMissingRequired.mockResolvedValue({ status: 'error', error });
    const onBrowseFor = view('missing-failed');
    await installFromPanel();

    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith(
        "Couldn't find balm automatically — opening search",
        [formatError(error as never)],
      ),
    );
    expect(onBrowseFor).toHaveBeenCalledWith('balm');
  });
});
