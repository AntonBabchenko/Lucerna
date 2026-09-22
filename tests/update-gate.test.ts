import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

// The update's own gate: Lucerna closes to install, and the exit hook force-kills every running
// game and server — so `runUpdate` refuses, before it asks the backend, while anything runs or
// while it cannot tell. A refusal is a warning with the way forward, never a "failure" that
// offers a manual download.
const h = vi.hoisted(() => ({ activeTasks: [] as { state: string }[] }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { updateInstall: vi.fn(), restartBlocked: vi.fn(), updateDismiss: vi.fn() },
  events: {
    downloadProgress: { listen: vi.fn() },
    updateInstallPhase: { listen: vi.fn() },
  },
}));
vi.mock('$lib/tasks/registry.svelte', () => ({
  taskList: () => h.activeTasks,
  isActiveTask: (t: { state: string }) => t.state === 'running',
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));

import { commands, events } from '$lib/ipc/bindings';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';
import { runUpdate, updateInstalling, updatePhase, updateState } from '$lib/update/state.svelte';

const INSTALLER_URL = 'https://cdn.example/Lucerna_x64-setup.exe';
const warnings = () => toastList().filter((t) => t.kind === 'warning');
const infos = () => toastList().filter((t) => t.kind === 'info');

beforeEach(() => {
  vi.clearAllMocks();
  h.activeTasks = [];
  updateInstalling.value = false;
  updatePhase.value = null;
  for (const t of [...toastList()]) dismiss(t.id);
  updateState.value = {
    installer: { url: INSTALLER_URL },
    release_url: 'https://github.com/x/releases/tag/v1',
  } as never;
  (events.downloadProgress.listen as Mock).mockResolvedValue(() => {});
  (events.updateInstallPhase.listen as Mock).mockResolvedValue(() => {});
  (commands.restartBlocked as Mock).mockResolvedValue('none');
  (commands.updateInstall as Mock).mockResolvedValue({ status: 'ok', data: null });
});

describe('runUpdate — the gate', () => {
  it.each([
    ['running', /Close Minecraft and stop your servers/],
    ['busy', /Wait for the running operation/],
    ['unknown', /couldn't check whether a game or a server is running/],
  ])('refuses while the backend answers %s, with the way forward and no action', async (answer, text) => {
    (commands.restartBlocked as Mock).mockResolvedValue(answer);
    await runUpdate();
    expect(commands.updateInstall).not.toHaveBeenCalled();
    expect(warnings()).toHaveLength(1);
    expect(warnings()[0].title).toMatch(text);
    expect(warnings()[0].action).toBeUndefined();
    expect(updateInstalling.value).toBe(false);
  });

  it('treats a rejection and a malformed answer as "could not tell"', async () => {
    (commands.restartBlocked as Mock).mockRejectedValue(new Error('ipc down'));
    await runUpdate();
    expect(commands.updateInstall).not.toHaveBeenCalled();
    expect(warnings()[0].title).toMatch(/couldn't check/);

    for (const t of [...toastList()]) dismiss(t.id);
    (commands.restartBlocked as Mock).mockResolvedValue('bogus');
    await runUpdate();
    expect(commands.updateInstall).not.toHaveBeenCalled();
    expect(warnings()[0].title).toMatch(/couldn't check/);
  });

  it('refuses while this window has a task in flight — the backend cannot see an instance being created', async () => {
    h.activeTasks = [{ state: 'running' }];
    await runUpdate();
    expect(commands.restartBlocked).not.toHaveBeenCalled();
    expect(commands.updateInstall).not.toHaveBeenCalled();
    expect(warnings()[0].title).toMatch(/Wait for the running operation/);
  });

  it('a finished task does not block', async () => {
    h.activeTasks = [{ state: 'done' }];
    await runUpdate();
    expect(commands.updateInstall).toHaveBeenCalledTimes(1);
  });

  it('still opens the release page on a notify-only platform, game or no game', async () => {
    updateState.value = { installer: null, release_url: 'https://github.com/x/releases' } as never;
    (commands.restartBlocked as Mock).mockResolvedValue('running');
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await runUpdate();
    // openExternalHttps dynamic-imports the opener; the call lands a tick after runUpdate returns.
    await vi.waitFor(() => expect(openUrl).toHaveBeenCalledWith('https://github.com/x/releases'));
    expect(commands.restartBlocked).not.toHaveBeenCalled();
    expect(warnings()).toHaveLength(0);
  });

  it('a backend refusal (something started during the download) is the same warning, not a failure', async () => {
    (commands.updateInstall as Mock).mockResolvedValue({
      status: 'error',
      error: { kind: 'update_blocked', block: 'running' },
    });
    await runUpdate();
    expect(warnings()).toHaveLength(1);
    expect(warnings()[0].title).toMatch(/Close Minecraft/);
    expect(warnings()[0].action).toBeUndefined();
    expect(updateInstalling.value).toBe(false);
  });
});

describe('runUpdate — honesty after the gate', () => {
  it('headlines only a failed verification as a verification failure', async () => {
    (commands.updateInstall as Mock).mockResolvedValue({
      status: 'error',
      error: { kind: 'update_verification_failed', details: 'signature' },
    });
    await runUpdate();
    expect(warnings()[0].title).toMatch(/Couldn't verify the update/);

    for (const t of [...toastList()]) dismiss(t.id);
    (commands.updateInstall as Mock).mockResolvedValue({
      status: 'error',
      error: { kind: 'update_install_failed', details: 'disk full' },
    });
    await runUpdate();
    expect(warnings()[0].title).toMatch(/Couldn't install the update/);
    expect(warnings()[0].title).not.toMatch(/verify/i);

    for (const t of [...toastList()]) dismiss(t.id);
    (commands.updateInstall as Mock).mockRejectedValue(new Error('ipc channel closed'));
    await runUpdate();
    expect(warnings()[0].title).toMatch(/Couldn't install the update/);
    expect(warnings()[0].action).toBeDefined(); // a real failure keeps "Open release page"
  });

  it('an install that returns without exiting (nothing newer any more) frees the button and says so', async () => {
    // Today the flag stayed set: "Installing…" and disabled for the rest of the session.
    (commands.updateInstall as Mock).mockResolvedValue({ status: 'ok', data: null });
    await runUpdate();
    expect(updateInstalling.value).toBe(false);
    expect(updatePhase.value).toBeNull();
    expect(updateState.value).toBeNull();
    expect(infos()[0].title).toMatch(/already up to date/);
  });

  it('follows the phase event into the button state and the toast, and tears both listeners down', async () => {
    let onPhase!: (e: { payload: { phase: string } }) => void;
    const unPhase = vi.fn();
    const unProgress = vi.fn();
    (events.updateInstallPhase.listen as Mock).mockImplementation((cb) => {
      onPhase = cb;
      return Promise.resolve(unPhase);
    });
    (events.downloadProgress.listen as Mock).mockResolvedValue(unProgress);
    let resolveInstall!: (v: unknown) => void;
    (commands.updateInstall as Mock).mockReturnValue(new Promise((r) => (resolveInstall = r)));

    const p = runUpdate();
    // Two listeners register behind two awaits (the gate, the progress listener): drain a
    // macrotask rather than counting microtasks.
    await new Promise((r) => setTimeout(r, 0));
    onPhase({ payload: { phase: 'verifying' } });
    expect(updatePhase.value).toBe('verifying');
    expect(toastList().find((t) => t.progress !== undefined)?.title).toMatch(/Verifying/);

    resolveInstall({
      status: 'error',
      error: { kind: 'update_install_failed', details: 'x' },
    });
    await p;
    expect(unPhase).toHaveBeenCalledTimes(1);
    expect(unProgress).toHaveBeenCalledTimes(1);
    expect(updatePhase.value).toBeNull();
  });

  it('a phase listener that cannot register aborts like a progress listener that cannot', async () => {
    (events.updateInstallPhase.listen as Mock).mockRejectedValue(new Error('ipc listen failed'));
    await runUpdate();
    expect(commands.updateInstall).not.toHaveBeenCalled();
    expect(warnings()).toHaveLength(1);
    expect(updateInstalling.value).toBe(false);
  });
});
