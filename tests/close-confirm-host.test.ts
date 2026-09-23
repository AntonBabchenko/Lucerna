// The close dialog: opened by the backend, acknowledged once it is on screen,
// and never able to kill anything the user did not confirm.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

type Payload = {
  generation: number;
  losses: { games: boolean; servers: number; operation: boolean; unchecked: boolean };
};

const h = vi.hoisted(() => ({
  listen: vi.fn(),
  ack: vi.fn(),
  confirm: vi.fn(),
  emit: null as null | ((e: { payload: Payload }) => void),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appCloseAskShown: (g: number) => h.ack(g),
    appConfirmClose: (g: number, l: unknown) => h.confirm(g, l),
  },
  events: { closeConfirmNeeded: { listen: h.listen } },
}));

import CloseConfirmHost from '$lib/close/CloseConfirmHost.svelte';

const GAME = { games: true, servers: 0, operation: false, unchecked: false };
const UNCHECKED = { games: false, servers: 0, operation: false, unchecked: true };

async function open(generation: number, losses: Payload['losses']) {
  if (!h.emit) throw new Error('the host never subscribed to closeConfirmNeeded');
  h.emit({ payload: { generation, losses } });
  await tick();
  await tick();
}

beforeEach(() => {
  vi.clearAllMocks();
  h.emit = null;
  h.listen.mockImplementation(async (cb: (e: { payload: Payload }) => void) => {
    h.emit = cb;
    return () => {};
  });
  h.ack.mockResolvedValue(true);
  h.confirm.mockResolvedValue({ status: 'ok', data: null });
});

describe('the close dialog', () => {
  it('opens when the backend asks, naming what closing would lose', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(1, GAME);
    expect(screen.getByRole('dialog', { name: 'Close Lucerna?' })).toBeTruthy();
    expect(screen.getByText(/Minecraft is still running/)).toBeTruthy();
  });

  it('tells the backend it is on screen, so the native fallback stands down', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(7, GAME);
    await waitFor(() => expect(h.ack).toHaveBeenCalledWith(7));
  });

  it('closes itself when the backend says the native dialog already took over', async () => {
    h.ack.mockResolvedValue(false);
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(2, GAME);
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('focuses Cancel, not the destructive button', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(1, GAME);
    await waitFor(() =>
      expect((document.activeElement as HTMLElement | null)?.textContent?.trim()).toBe('Cancel'),
    );
  });

  it('cancels on Escape without asking the backend to close anything', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(1, GAME);
    await fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(h.confirm).not.toHaveBeenCalled();
  });

  it('confirms with exactly what it showed', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(3, GAME);
    await fireEvent.click(screen.getByRole('button', { name: /Close everything/ }));
    await waitFor(() => expect(h.confirm).toHaveBeenCalledWith(3, GAME));
  });

  it('styles the confirm as destructive only when something is killed', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(1, GAME);
    expect(screen.getByRole('button', { name: /Close everything/ }).className).toContain(
      'btn-danger',
    );
    await open(2, UNCHECKED);
    const confirm = screen.getByRole('button', { name: /Close Lucerna/ });
    expect(confirm.className).toContain('btn-primary');
    expect(confirm.className).not.toContain('btn-danger');
  });

  it('replaces an open ask instead of stacking a second dialog', async () => {
    render(CloseConfirmHost);
    await waitFor(() => expect(h.listen).toHaveBeenCalled());
    await open(1, GAME);
    await open(2, { ...GAME, servers: 2 });
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByText(/2 servers are still running/)).toBeTruthy();
  });
});
