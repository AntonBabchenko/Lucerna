import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import ServersView from '$lib/servers/ServersView.svelte';

// vi.mock factories are hoisted above imports — use vi.hoisted so the shared
// mutable state and vi.fn() references are available inside the factory.
const { mockList, mockRemove } = vi.hoisted(() => {
  const mockList: ServerWithStatus_Serialize[] = [
    {
      id: 'srv-1',
      name: 'My Server',
      mc_version: '1.21.1',
      loader: 'fabric' as const,
      loader_version: null as string | null,
      max_heap_mb: 2048,
      extra_jvm_args: '',
      created_unix_ms: null as number | null,
      eula_accepted: true,
      created_from_instance: null as string | null,
      running: false,
      pid: null as number | null,
      port: null as number | null,
      upload: null,
      upload_password_set: false,
    },
  ];
  const mockRemove = vi.fn().mockResolvedValue({ ok: true });
  return { mockList, mockRemove };
});

// Mock bindings so IPC never fires.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {},
  events: {
    serverLogLine: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverSpawned: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverUploadProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return mockList;
    },
    running: (id: string) => mockList.find((s) => s.id === id)?.running ?? false,
    refresh: vi.fn().mockResolvedValue(undefined),
    init: vi.fn(),
    remove: mockRemove,
  },
}));

function baseProps() {
  return { instances: [], versions: [], onInstanceCreated: () => {} };
}

describe('ServersView delete affordance', () => {
  beforeAll(() => locale.set('en'));

  it('(a) Delete button is disabled when the server is running', () => {
    mockList[0].running = true;

    render(ServersView, baseProps());

    const deleteBtn = screen.getByRole('button', {
      name: 'Delete server',
    }) as HTMLButtonElement;
    expect(deleteBtn.disabled).toBe(true);

    mockList[0].running = false;
  });

  it('(b) clicking Delete when stopped shows confirm dialog; confirming calls serverState.remove', async () => {
    mockList[0].running = false;
    mockRemove.mockClear();

    render(ServersView, baseProps());

    // Delete button should be enabled
    const deleteBtn = screen.getByRole('button', { name: 'Delete server' }) as HTMLButtonElement;
    expect(deleteBtn.disabled).toBe(false);

    // Click the delete trigger — dialog should appear
    await fireEvent.click(deleteBtn);

    // Dialog should now be visible
    expect(screen.getByRole('dialog')).toBeTruthy();

    // Click the confirm button inside the dialog
    const confirmBtn = screen.getByRole('button', { name: 'Delete' });
    await fireEvent.click(confirmBtn);

    expect(mockRemove).toHaveBeenCalledWith('srv-1');
  });

  it('(c) clicking Cancel in the dialog does NOT call serverState.remove', async () => {
    mockList[0].running = false;
    mockRemove.mockClear();

    render(ServersView, baseProps());

    const deleteBtn = screen.getByRole('button', { name: 'Delete server' }) as HTMLButtonElement;
    await fireEvent.click(deleteBtn);

    // Dialog should be open
    expect(screen.getByRole('dialog')).toBeTruthy();

    // Click Cancel
    const cancelBtn = screen.getByRole('button', { name: 'Cancel' });
    await fireEvent.click(cancelBtn);

    expect(mockRemove).not.toHaveBeenCalled();
  });
});
