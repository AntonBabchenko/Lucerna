import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  getDataLocation: vi.fn(),
  cancelDataLocationMove: vi.fn(),
  retryDataMoveCleanup: vi.fn(),
  openDataMoveLeftovers: vi.fn(),
  restartLauncher: vi.fn(),
  listen: vi.fn(),
  pushInfo: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    getDataLocation: h.getDataLocation,
    cancelDataLocationMove: h.cancelDataLocationMove,
    retryDataMoveCleanup: h.retryDataMoveCleanup,
    openDataMoveLeftovers: h.openDataMoveLeftovers,
    restartLauncher: h.restartLauncher,
  },
  events: { dataMigrationProgress: { listen: h.listen } },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushInfo: h.pushInfo }));

import DataMoveHost from '$lib/settings/DataMoveHost.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';

const flush = () => new Promise((r) => setTimeout(r, 0));
const IDLE = { kind: 'idle' };
const RUNNING = { kind: 'running', phase: 'verifying' };
// The full generated `restart_required` shape; `retry_possible` is the backend's verdict and the
// only thing that offers "Try again".
const FINAL = {
  kind: 'restart_required',
  old_root: 'C:\\Old',
  new_root: 'D:\\New',
  leftovers: ['logs'],
  old_root_intact: false,
  old_root_is_default: false,
  retry_possible: true,
};
const IO_ERROR = { status: 'error', error: { kind: 'io', path: 'x', details: 'ipc down' } };
const ok = (relocation: unknown) => ({
  status: 'ok',
  data: {
    effective: 'C:\\Old',
    configured: null,
    fell_back: false,
    default_dir: 'C:\\Default',
    relocation,
  },
});

/** Drive the singleton to a known backend state, then mount the host. */
async function show(relocation: unknown) {
  h.getDataLocation.mockResolvedValue(ok(relocation));
  await dataLocation.refresh();
  dataLocation.ackOrphanEnded();
  const utils = render(DataMoveHost);
  await flush();
  return utils;
}
const button = (name: string) => screen.queryByRole('button', { name }) as HTMLButtonElement | null;
const click = async (name: string) => {
  await fireEvent.click(button(name) as HTMLButtonElement);
  await flush();
};

beforeEach(() => {
  vi.clearAllMocks();
  h.listen.mockResolvedValue(() => {});
  // Not wrapped in typedError: it resolves to nothing, and a failure is a rejection.
  h.cancelDataLocationMove.mockResolvedValue(undefined);
  for (const cmd of [h.openDataMoveLeftovers, h.restartLauncher])
    cmd.mockResolvedValue({ status: 'ok', data: null });
});

describe('DataMoveHost', () => {
  it('renders nothing while no move is in progress', async () => {
    await show(IDLE);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('re-shows a running move after a remount (F5, tab switch)', async () => {
    const first = await show(RUNNING);
    expect(screen.getByTestId('data-move-phase').textContent).toBe('Verifying copy…');
    first.unmount();
    expect(screen.queryByRole('dialog')).toBeNull();
    render(DataMoveHost);
    await flush();
    expect(screen.getByTestId('data-move-phase').textContent).toBe('Verifying copy…');
  });

  it('re-shows the final state after a remount — it is the only way to restart', async () => {
    const first = await show(FINAL);
    expect(button('Restart')).not.toBeNull();
    first.unmount();
    render(DataMoveHost);
    await flush();
    expect(button('Restart')).not.toBeNull();
    expect(screen.getByText(/could not remove 1 item/)).toBeTruthy();
  });

  it('requests a cancel once, stays busy, and reports a run that ended without its owner', async () => {
    await show(RUNNING);
    await click('Cancel move');
    expect(h.cancelDataLocationMove).toHaveBeenCalledTimes(1);
    // By test id: a busy BusyButton's accessible name gains its spinner's "Loading…".
    expect(screen.getByTestId('data-move-cancel').getAttribute('aria-busy')).toBe('true');
    // The backend cleaned up and went back to idle. This page never awaited the command (it
    // models a reloaded page), so the host says what happened.
    h.getDataLocation.mockResolvedValue(ok(IDLE));
    await dataLocation.refresh();
    await flush();
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(h.pushInfo).toHaveBeenCalledWith(expect.stringContaining('stopped before the switch'));
  });

  it('shows a failed cancel request and lets the user try again', async () => {
    h.cancelDataLocationMove.mockRejectedValue(new Error('ipc down'));
    await show(RUNNING);
    await click('Cancel move');
    expect(screen.getByText("Couldn't cancel the move: ipc down")).toBeTruthy();
    expect(button('Cancel move')?.disabled).toBe(false);
  });

  it('applies the result of Try again without another status read', async () => {
    // Everything a retry could remove is gone: the backend withdraws `retry_possible` with it.
    h.retryDataMoveCleanup.mockResolvedValue({
      status: 'ok',
      data: { ...FINAL, leftovers: [], retry_possible: false },
    });
    await show(FINAL);
    const reads = h.getDataLocation.mock.calls.length;
    await click('Try again');
    expect(screen.getByText('The leftovers in "C:\\Old" have been removed.')).toBeTruthy();
    expect(button('Try again')).toBeNull();
    expect(h.getDataLocation.mock.calls.length).toBe(reads);
  });

  it('shows a failed retry and keeps the leftovers listed', async () => {
    h.retryDataMoveCleanup.mockResolvedValue(IO_ERROR);
    await show(FINAL);
    await click('Try again');
    expect(screen.getByText(/Couldn't remove the leftovers: /)).toBeTruthy();
    expect(button('Try again')?.disabled).toBe(false);
  });

  it('opens the recorded old folder — the frontend passes no path', async () => {
    await show(FINAL);
    await click('Open old folder');
    expect(h.openDataMoveLeftovers).toHaveBeenCalledWith();
  });

  it.each([
    ['a refusal', async () => ({ status: 'error', error: { kind: 'data_location_busy' } })],
    ['a thrown error', async () => Promise.reject(new Error('ipc down'))],
  ])('tells the user how to get out when Restart fails with %s', async (_label, impl) => {
    h.restartLauncher.mockImplementation(impl);
    await show(FINAL);
    await click('Restart');
    expect(screen.getByText(/Close Lucerna and open it again/)).toBeTruthy();
    expect(button('Restart')?.disabled).toBe(false);
  });

  it('polls the status while a move runs, because nothing else reports its end', async () => {
    vi.useFakeTimers();
    try {
      h.getDataLocation.mockResolvedValue(ok(RUNNING));
      await dataLocation.refresh();
      render(DataMoveHost);
      await vi.advanceTimersByTimeAsync(0);
      const reads = h.getDataLocation.mock.calls.length;
      await vi.advanceTimersByTimeAsync(1600);
      expect(h.getDataLocation.mock.calls.length).toBeGreaterThan(reads);
    } finally {
      vi.useRealTimers();
    }
  });

  it('is mounted from +page.svelte after every other modal, so it paints on top', () => {
    const src = readFileSync(resolve('src/routes/+page.svelte'), 'utf8');
    const host = src.indexOf('<DataMoveHost />');
    expect(host).toBeGreaterThan(-1);
    for (const earlier of ['<SettingsModal />', '<WhatsNewModal />', '<MicrosoftSigningInModal']) {
      expect(host).toBeGreaterThan(src.indexOf(earlier));
    }
  });
});
