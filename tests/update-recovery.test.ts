import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushInfo: vi.fn(),
  pushWarning: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: { takeUpdateRecoveryReport: vi.fn() } }));

import { locale } from '$lib/i18n';
import type { RecoveredUpdate } from '$lib/ipc/bindings';
import {
  __resetUpdateRecoveryForTest,
  announceUpdateRecovery,
} from '$lib/modpacks/update-recovery.svelte';
import { pushInfo, pushWarning } from '$lib/toasts/toasts.svelte';

const REPORT: RecoveredUpdate[] = [
  { instance_id: 'a', instance_name: 'Create+', outcome: { kind: 'restored' } },
  {
    instance_id: 'b',
    instance_name: 'Vanilla+',
    outcome: { kind: 'incomplete', folder: 'C:/x/1000-abcdef12-kept', details: 'mods/a.jar' },
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
