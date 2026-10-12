import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  openInstanceFolderReporting,
  openModsFolderReporting,
  reportActivationFailure,
} from '$lib/instances/instance-actions';

const m = vi.hoisted(() => ({
  openInstanceFolder: vi.fn(),
  openModsFolder: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { openInstanceFolder: m.openInstanceFolder, openModsFolder: m.openModsFolder },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushWarning: m.pushWarning }));

const ioError = { status: 'error', error: { kind: 'io', path: '/x', message: 'denied' } };

beforeEach(() => vi.clearAllMocks());

describe('row-menu actions report failures as toasts', () => {
  it('a folder that cannot be opened says so, naming the reason', async () => {
    m.openInstanceFolder.mockResolvedValue(ioError);
    await openInstanceFolderReporting('a');
    expect(m.openInstanceFolder).toHaveBeenCalledWith('a');
    expect(m.pushWarning).toHaveBeenCalledTimes(1);
    const [title, lines] = m.pushWarning.mock.calls[0];
    expect(title).toBe("Couldn't open the instance folder.");
    expect(lines).toHaveLength(1);
  });

  it('an opened folder raises nothing', async () => {
    m.openInstanceFolder.mockResolvedValue({ status: 'ok', data: null });
    await openInstanceFolderReporting('a');
    expect(m.pushWarning).not.toHaveBeenCalled();
  });

  it('the mods folder reports with its own title', async () => {
    m.openModsFolder.mockResolvedValue(ioError);
    await openModsFolderReporting('a');
    expect(m.openModsFolder).toHaveBeenCalledWith('a');
    expect(m.pushWarning.mock.calls[0][0]).toBe("Couldn't open the mods folder.");
  });

  it('a failed switch names the instance and carries the reason', () => {
    reportActivationFailure('Pack', 'boom');
    expect(m.pushWarning).toHaveBeenCalledWith("Couldn't switch to Pack", ['boom']);
  });
});
