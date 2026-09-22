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

// Its own file on purpose: `dataLocation` is a module singleton and the first case needs one that
// has NEVER read a status. The cases run in order and build on each other.
const status = (fell_back: boolean) => ({
  status: 'ok',
  data: {
    effective: 'C:\\X',
    configured: fell_back ? 'D:\\Gone' : null,
    fell_back,
    fallback: fell_back ? { kind: 'root_missing' } : null,
    default_dir: 'C:\\Default',
    relocation: { kind: 'idle' },
  },
});

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
    const { dataLocation } = await import('$lib/settings/data-location.svelte');
    h.getDataLocation.mockResolvedValue(status(false));
    await dataLocation.refresh();
    await initOnboarding();
    expect(tourState.active).toBe(true);
  });
});
