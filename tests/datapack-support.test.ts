// The 1.13 datapack gate's shared answer (spec 2026-09-24 §4 U2): AddonsTab's
// kind switch and WorldsTab's detail dialog read the same session cache.
import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({ commands: { instanceSupportsDatapacks: vi.fn() } }));

import { commands } from '$lib/ipc/bindings';
import {
  cachedDatapackSupport,
  rememberDatapackSupport,
  resolveDatapackSupport,
} from '$lib/mods/datapack-support';

describe('datapack support gate', () => {
  it('reads as supported until an answer is remembered', () => {
    expect(cachedDatapackSupport('never-asked', '1.12.2')).toBe(true);
  });

  it('remembers an answer per instance AND Minecraft version', () => {
    rememberDatapackSupport('i1', '1.12.2', false);
    expect(cachedDatapackSupport('i1', '1.12.2')).toBe(false);
    // The Manage modal changes a version in place; the old answer must not follow.
    expect(cachedDatapackSupport('i1', '1.21.1')).toBe(true);
  });

  it('an IPC error answers supported: uncertainty must not hide the feature', async () => {
    vi.mocked(commands.instanceSupportsDatapacks).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: 'instance.json', details: 'denied' },
    });
    expect(await resolveDatapackSupport('i2')).toBe(true);
  });

  it("passes the backend's verdict through", async () => {
    vi.mocked(commands.instanceSupportsDatapacks).mockResolvedValueOnce({
      status: 'ok',
      data: false,
    });
    expect(await resolveDatapackSupport('i3')).toBe(false);
  });
});
