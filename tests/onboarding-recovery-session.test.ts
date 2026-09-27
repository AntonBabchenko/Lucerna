import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  appSettingsGet: vi.fn(),
  getDataLocation: vi.fn(),
  listen: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { appSettingsGet: h.appSettingsGet, getDataLocation: h.getDataLocation },
  events: { dataMigrationProgress: { listen: h.listen } },
}));

import { initOnboarding, tourState } from '$lib/onboarding/state.svelte';
import { serversUi } from '$lib/servers/servers-ui.svelte';
import { dataLocation, startupSurfacesMustWait } from '$lib/settings/data-location.svelte';

// Its own file on purpose: `dataLocation` is a module singleton and the first case needs one that
// has NEVER read a status. The cases run in order and build on each other.
const status = (fell_back: boolean, relocation: object = { kind: 'idle' }) => ({
  status: 'ok',
  data: {
    effective: 'C:\\X',
    configured: fell_back ? 'D:\\Gone' : null,
    fell_back,
    fallback: fell_back ? { kind: 'root_missing' } : null,
    default_dir: 'C:\\Default',
    relocation,
  },
});

const RESTART_REQUIRED = {
  kind: 'restart_required',
  old_root: 'C:\\X',
  new_root: 'E:\\LucernaData',
  leftovers: ['instances'],
  old_root_intact: false,
  old_root_is_default: true,
  retry_possible: true,
};

beforeEach(() => {
  vi.clearAllMocks();
  h.appSettingsGet.mockResolvedValue({
    status: 'ok',
    data: { onboarding: { tour_completed_version: null } },
  });
  tourState.active = false;
  tourState.currentStep = 0;
  tourState.contextual = false;
  serversUi.setMode('client');
});

describe('initOnboarding in a recovery session', () => {
  it('does NOT start the tour while it cannot tell which session this is', async () => {
    // "Could not tell" is restrictive here: a first-run tour over the recovery banner teaches
    // create and Play while both are refused. Nothing is lost — it is offered again next start.
    h.getDataLocation.mockRejectedValue(new Error('ipc channel closed'));
    await initOnboarding();
    expect(h.getDataLocation).toHaveBeenCalled();
    expect(tourState.active).toBe(false);
  });

  it('does NOT start the tour in a recovery session', async () => {
    h.getDataLocation.mockResolvedValue(status(true));
    await initOnboarding();
    expect(tourState.active).toBe(false);
  });

  it('starts it as before once the launcher runs from its data folder', async () => {
    // The store latched the recovery status above; a fresh read is forced through refresh().
    h.getDataLocation.mockResolvedValue(status(false));
    await dataLocation.refresh();
    await initOnboarding();
    expect(tourState.active).toBe(true);
  });
});

describe('initOnboarding while a data move is in flight', () => {
  it('does NOT start the tour while a finished move waits for the restart', async () => {
    // A reload (F5) in the move's final state: this process still reads the OLD root, whose
    // app.json the move already deleted, so every setting is its default and
    // `tour_completed_version` reads "never". The tour would open over the restart dialog.
    h.getDataLocation.mockResolvedValue(status(false, RESTART_REQUIRED));
    await dataLocation.refresh();
    expect(await startupSurfacesMustWait()).toBe(true);
    await initOnboarding();
    expect(h.appSettingsGet).not.toHaveBeenCalled();
    expect(tourState.active).toBe(false);
  });

  it('does NOT start the tour while a move is copying', async () => {
    h.getDataLocation.mockResolvedValue(status(false, { kind: 'running', phase: 'copying' }));
    await dataLocation.refresh();
    expect(await startupSurfacesMustWait()).toBe(true);
    await initOnboarding();
    expect(tourState.active).toBe(false);
  });

  it('starts it again once no move is in flight', async () => {
    h.getDataLocation.mockResolvedValue(status(false));
    await dataLocation.refresh();
    expect(await startupSurfacesMustWait()).toBe(false);
    await initOnboarding();
    expect(tourState.active).toBe(true);
  });
});
