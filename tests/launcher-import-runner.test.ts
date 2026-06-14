import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    launcherImportRun: vi.fn(),
  },
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `err:${e.kind}`),
}));

import { commands } from '$lib/ipc/bindings';
import type { ForeignInstance } from '$lib/ipc/bindings';
import { runLauncherImport } from '$lib/instances/import/launcher-import-runner';

const mockForeign: ForeignInstance = {
  source: 'prism',
  name: 'My Pack',
  root: '/prism/instances/my-pack',
  minecraft_dir: '/prism/instances/my-pack/.minecraft',
  mc_version: '1.20.4',
  loader: 'fabric',
  loader_version: '0.15.7',
  max_heap_mb: 4096,
  extra_jvm_args: null,
  content: [
    { category: 'mods', file_count: 42, total_bytes: 1024 * 1024 * 100 },
    { category: 'config', file_count: 5, total_bytes: null },
  ],
  known_mods: [],
};

const mockInstance = {
  id: 'inst-abc',
  name: 'My Pack',
  mc_version: '1.20.4',
  loader: 'fabric' as const,
  loader_version: '0.15.7',
  max_heap_mb: 4096,
  extra_jvm_args: null,
  installed: true,
  imported_from: null,
  content_overview: null,
  integrity: null,
  playtime: null,
  gpu_preference: null,
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe('runLauncherImport', () => {
  it('calls launcherImportRun with the right arguments', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: mockInstance,
    });

    await runLauncherImport(
      { foreign: mockForeign, selected: ['mods', 'config'], targetName: 'My Pack' },
      () => {},
    );

    expect(commands.launcherImportRun).toHaveBeenCalledOnce();
    const args = (commands.launcherImportRun as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(args[0]).toBe(mockForeign);
    expect(args[1]).toEqual(['mods', 'config']);
    expect(args[2]).toBe('My Pack');
    expect(args[3]).toBeNull(); // mcVersionOverride
    expect(args[4]).toBeNull(); // loaderOverride
    expect(args[5]).toBeNull(); // loaderVersionOverride
    expect(args[6]).toBeDefined(); // Channel
  });

  it('returns ok outcome with name and instanceId', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: mockInstance,
    });

    const outcome = await runLauncherImport(
      { foreign: mockForeign, selected: ['mods'], targetName: 'My Pack' },
      () => {},
    );

    expect(outcome).toEqual({
      status: 'ok',
      name: 'My Pack',
      instanceId: 'inst-abc',
      untrackedMods: 0,
    });
  });

  it('picks up untrackedMods from the done phase', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockImplementation(
      async (_f, _s, _n, _mv, _lo, _lv, ch: { onmessage: ((m: unknown) => void) | null }) => {
        ch.onmessage?.({ phase: 'creating_instance', name: 'My Pack' });
        ch.onmessage?.({ phase: 'done', instance_id: 'inst-abc', untracked_mods: 3 });
        return { status: 'ok', data: mockInstance };
      },
    );

    const outcome = await runLauncherImport(
      { foreign: mockForeign, selected: ['mods'], targetName: 'My Pack' },
      () => {},
    );

    expect(outcome).toMatchObject({ status: 'ok', untrackedMods: 3 });
  });

  it('calls onProgress with null then with each phase message', async () => {
    const phases: Array<unknown> = [];
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockImplementation(
      async (_f, _s, _n, _mv, _lo, _lv, ch: { onmessage: ((m: unknown) => void) | null }) => {
        ch.onmessage?.({ phase: 'creating_instance', name: 'My Pack' });
        ch.onmessage?.({ phase: 'done', instance_id: 'inst-abc', untracked_mods: 0 });
        return { status: 'ok', data: mockInstance };
      },
    );

    await runLauncherImport(
      { foreign: mockForeign, selected: ['mods'], targetName: 'My Pack' },
      (p) => phases.push(p),
    );

    expect(phases[0]).toBeNull();
    expect(phases[1]).toMatchObject({ phase: 'creating_instance' });
    expect(phases[2]).toMatchObject({ phase: 'done' });
  });

  it('returns error outcome when command fails', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '/x', details: 'nope' },
    });

    const outcome = await runLauncherImport(
      { foreign: mockForeign, selected: ['mods'], targetName: 'My Pack' },
      () => {},
    );

    expect(outcome).toEqual({ status: 'error', message: 'err:io' });
  });

  it('passes mcVersionOverride/loaderOverride through when provided', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: mockInstance,
    });

    await runLauncherImport(
      {
        foreign: mockForeign,
        selected: ['mods'],
        targetName: 'My Pack',
        mcVersionOverride: '1.21',
        loaderOverride: 'fabric',
        loaderVersionOverride: '0.16.0',
      },
      () => {},
    );

    const args = (commands.launcherImportRun as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(args[3]).toBe('1.21');
    expect(args[4]).toBe('fabric');
    expect(args[5]).toBe('0.16.0');
  });
});
