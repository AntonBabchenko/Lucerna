import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    transferModData: vi.fn(),
  },
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `err:${e.kind}`),
}));

import { runTransfer } from '$lib/instances/transfer/transfer-runner';
import { commands } from '$lib/ipc/bindings';

const request = {
  sourceId: 'inst-2',
  sourceName: 'Old',
  targetId: 'inst-1',
  targetName: 'New',
  selection: { items: ['schematics' as const], other: [] },
  onConflict: 'replace' as const,
};

const outcome = { copied: 2, replaced: 0, kept_existing: 0, skipped: 0, replaced_kept_at: null };

beforeEach(() => {
  vi.clearAllMocks();
});

describe('runTransfer', () => {
  it('reports null, then each streamed phase, then the outcome', async () => {
    const phases: Array<unknown> = [];
    (commands.transferModData as ReturnType<typeof vi.fn>).mockImplementation(
      async (_s, _t, _sel, _c, ch: { onmessage: ((m: unknown) => void) | null }) => {
        ch.onmessage?.({ phase: 'copying', current: 1, total: 2 });
        ch.onmessage?.({ phase: 'placing', current: 2, total: 2 });
        return { status: 'ok', data: outcome };
      },
    );
    const r = await runTransfer(request, (p) => phases.push(p));
    expect(phases[0]).toBeNull();
    expect(phases[1]).toMatchObject({ phase: 'copying' });
    expect(phases[2]).toMatchObject({ phase: 'placing' });
    expect(r).toEqual({ status: 'ok', outcome });
    expect(commands.transferModData).toHaveBeenCalledWith(
      'inst-2',
      'inst-1',
      request.selection,
      'replace',
      expect.anything(),
    );
  });

  it('turns a backend error into its message', async () => {
    (commands.transferModData as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_busy' },
    } as never);
    expect(await runTransfer(request, () => {})).toEqual({
      status: 'error',
      message: 'err:instance_busy',
    });
  });
});
