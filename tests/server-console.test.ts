import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerConsole from '$lib/servers/ServerConsole.svelte';

// Mock bindings so IPC never fires.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverSendCommand: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

// Mock the server-state store so tests control lines() and running() directly.
const mockLines: Record<string, string[]> = {};
const mockRunning: Record<string, boolean> = {};

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [];
    },
    lines: (id: string) => mockLines[id] ?? [],
    running: (id: string) => mockRunning[id] ?? false,
    refresh: vi.fn().mockResolvedValue(undefined),
    clearLines: vi.fn(),
    init: vi.fn(),
  },
}));

describe('ServerConsole', () => {
  beforeAll(() => locale.set('en'));

  it('shows lines from the store when running', () => {
    mockLines['srv-1'] = ['[INFO] Server started', '[INFO] Done!'];
    mockRunning['srv-1'] = true;

    render(ServerConsole, { props: { serverId: 'srv-1' } });

    expect(screen.getByText('[INFO] Server started')).toBeTruthy();
    expect(screen.getByText('[INFO] Done!')).toBeTruthy();
  });

  it('shows empty state when no lines', () => {
    mockLines['srv-2'] = [];
    mockRunning['srv-2'] = true;

    render(ServerConsole, { props: { serverId: 'srv-2' } });

    expect(screen.getByText('No output yet')).toBeTruthy();
  });

  it('disables input and send button when server is not running', () => {
    mockLines['srv-3'] = [];
    mockRunning['srv-3'] = false;

    render(ServerConsole, { props: { serverId: 'srv-3' } });

    const input = screen.getByPlaceholderText(
      'Type a command (e.g. say hi, op Steve, stop)',
    ) as HTMLInputElement;
    expect(input.disabled).toBe(true);

    const sendBtn = screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement;
    expect(sendBtn.disabled).toBe(true);
  });

  it('shows notRunning hint when server is stopped', () => {
    mockLines['srv-4'] = [];
    mockRunning['srv-4'] = false;

    render(ServerConsole, { props: { serverId: 'srv-4' } });

    expect(screen.getByText('Start the server to use the console')).toBeTruthy();
  });

  it('send button is disabled when input is empty even if running', async () => {
    mockLines['srv-5'] = [];
    mockRunning['srv-5'] = true;

    render(ServerConsole, { props: { serverId: 'srv-5' } });

    const sendBtn = screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement;
    // Input is empty → Send should be disabled
    expect(sendBtn.disabled).toBe(true);
  });

  it('send button becomes enabled once the user types something (when running)', async () => {
    mockLines['srv-6'] = [];
    mockRunning['srv-6'] = true;

    render(ServerConsole, { props: { serverId: 'srv-6' } });

    const input = screen.getByPlaceholderText(
      'Type a command (e.g. say hi, op Steve, stop)',
    ) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'say hello' } });

    const sendBtn = screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement;
    expect(sendBtn.disabled).toBe(false);
  });
});
