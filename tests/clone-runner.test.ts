import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    cloneInstance: vi.fn(),
  },
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `err:${e.kind}`),
}));

import { runClone } from '$lib/instances/clone-runner';
import { commands } from '$lib/ipc/bindings';

const request = {
  sourceId: 'inst-1',
  newName: 'Default (copy)',
  options: {
    saves: true,
    settings: true,
    packs: false,
    config: true,
    options_txt: true,
    playtime: false,
  },
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe('runClone', () => {
  it('calls cloneInstance with source id, name, options and a channel', async () => {
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: { id: 'clone-1', name: 'Default (copy)' },
    });

    await runClone(request, () => {});

    expect(commands.cloneInstance).toHaveBeenCalledOnce();
    const args = (commands.cloneInstance as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(args[0]).toBe('inst-1');
    expect(args[1]).toBe('Default (copy)');
    expect(args[2]).toEqual(request.options);
    expect(args[3]).toBeDefined(); // Channel
  });

  it('returns ok outcome with the clone id and name', async () => {
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: { id: 'clone-1', name: 'Default (copy)' },
    });

    const outcome = await runClone(request, () => {});

    expect(outcome).toEqual({ status: 'ok', instanceId: 'clone-1', name: 'Default (copy)' });
  });

  it('calls onProgress with null then each streamed phase', async () => {
    const phases: Array<unknown> = [];
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockImplementation(
      async (_s, _n, _o, ch: { onmessage: ((m: unknown) => void) | null }) => {
        ch.onmessage?.({ category: 'mods', current: 1, total: 3 });
        ch.onmessage?.({ category: 'saves', current: 2, total: 8 });
        return { status: 'ok', data: { id: 'clone-1', name: 'Default (copy)' } };
      },
    );

    await runClone(request, (p) => phases.push(p));

    expect(phases[0]).toBeNull();
    expect(phases[1]).toMatchObject({ category: 'mods' });
    expect(phases[2]).toMatchObject({ category: 'saves' });
  });

  it('returns error outcome with the formatted message when the command fails', async () => {
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_busy' },
    });

    const outcome = await runClone(request, () => {});

    expect(outcome).toEqual({ status: 'error', message: 'err:instance_busy' });
  });
});
