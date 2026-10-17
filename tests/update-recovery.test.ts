import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushInfo: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { takeUpdateRecoveryReport: vi.fn(), openKeptFolder: vi.fn() },
}));

import { locale } from '$lib/i18n';
import type { RecoveredUpdate } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import {
  __resetUpdateRecoveryForTest,
  announceUpdateRecovery,
} from '$lib/modpacks/update-recovery.svelte';
import { pushActionToast, pushInfo, pushWarning } from '$lib/toasts/toasts.svelte';

const REPORT: RecoveredUpdate[] = [
  {
    instance_id: 'a',
    instance_name: 'Create+',
    outcome: { kind: 'restored' },
    operation: { kind: 'update' },
  },
  {
    instance_id: 'b',
    instance_name: 'Vanilla+',
    outcome: { kind: 'incomplete', folder: 'C:/x/1000-abcdef12-kept', details: 'mods/a.jar' },
    operation: { kind: 'update' },
  },
];

describe('announceUpdateRecovery', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    __resetUpdateRecoveryForTest();
  });

  it('says which profile was put back and which could not be, naming the kept folder', async () => {
    await announceUpdateRecovery({ take: async () => REPORT });

    expect(pushInfo).toHaveBeenCalledTimes(1);
    expect(vi.mocked(pushInfo).mock.calls[0][0]).toContain('Create+');
    expect(pushWarning).toHaveBeenCalledTimes(1);
    const warning = vi.mocked(pushWarning).mock.calls[0][0];
    expect(warning).toContain('Vanilla+');
    expect(warning).toContain('1000-abcdef12-kept');
  });

  it('names an interrupted data copy as a copy, with its source', async () => {
    await announceUpdateRecovery({
      take: async () => [
        {
          instance_id: 'c',
          instance_name: 'New pack',
          outcome: { kind: 'restored' },
          operation: { kind: 'data_copy', source: 'Old pack' },
        },
      ],
    });
    const info = vi.mocked(pushInfo).mock.calls[0][0];
    expect(info).toContain('Old pack');
    expect(info).toContain('New pack');
    expect(info).not.toContain('An update');
  });

  it('names the folder kept files were moved to, with Open folder, not a full path', async () => {
    vi.mocked(commands.openKeptFolder).mockResolvedValue({ status: 'ok', data: null });
    await announceUpdateRecovery({
      take: async () => [
        {
          instance_id: 'c',
          instance_name: 'New pack',
          outcome: { kind: 'kept_aside', folder: 'C:\\i\\lucerna\\replaced\\Replaced (2)' },
          operation: { kind: 'unknown' },
        },
      ],
    });
    expect(pushWarning).not.toHaveBeenCalled();
    const [kind, title, action] = vi.mocked(pushActionToast).mock.calls[0];
    expect(kind).toBe('info');
    expect(title).toContain('New pack');
    expect(title).toContain('Replaced (2)');
    expect(title).not.toContain('lucerna');
    expect(title).not.toMatch(/interrupted|undone/);
    expect(action.label).toBe('Open folder');
    action.run();
    expect(commands.openKeptFolder).toHaveBeenCalledWith('c', 'Replaced (2)');
  });

  it('warns, naming the folder, when those files could not be moved', async () => {
    await announceUpdateRecovery({
      take: async () => [
        {
          instance_id: 'c',
          instance_name: 'New pack',
          outcome: { kind: 'kept_in_place', folder: 'C:/i/txn/1-a-kept', details: 'denied' },
          operation: { kind: 'unknown' },
        },
      ],
    });
    const warning = vi.mocked(pushWarning).mock.calls[0][0];
    expect(warning).toContain('New pack');
    expect(warning).toContain('C:/i/txn/1-a-kept');
    expect(warning).not.toMatch(/undone/);
  });

  it('never calls a change it cannot name an update', async () => {
    await announceUpdateRecovery({
      take: async () => [
        {
          instance_id: 'd',
          instance_name: 'Pack',
          outcome: { kind: 'incomplete', folder: 'C:/i/txn/2-b-kept', details: 'eof' },
          operation: { kind: 'unknown' },
        },
      ],
    });
    const warning = vi.mocked(pushWarning).mock.calls[0][0];
    expect(warning).toContain('C:/i/txn/2-b-kept');
    expect(warning).not.toMatch(/update/i);
  });

  it('speaks once per launch', async () => {
    await announceUpdateRecovery({ take: async () => REPORT });
    await announceUpdateRecovery({ take: async () => REPORT });
    expect(pushInfo).toHaveBeenCalledTimes(1);
  });

  it('stays quiet when the report cannot be read', async () => {
    await announceUpdateRecovery({
      take: async () => {
        throw new Error('bridge died');
      },
    });
    expect(pushInfo).not.toHaveBeenCalled();
    expect(pushWarning).not.toHaveBeenCalled();
  });
});
