import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => {
  return {
    commands: {
      verifyInstance: vi.fn(),
      repairInstance: vi.fn(),
    },
    events: {
      verifyProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
    },
  };
});

import { createIntegrity } from '$lib/instances/integrity.svelte';
import { commands } from '$lib/ipc/bindings';

const healthy = {
  instance_id: 'i',
  effective_version_id: '1.20.4',
  categories: [],
  problems: [],
  healthy: true,
  manifest_recoverable: false,
};

const broken = {
  ...healthy,
  healthy: false,
  problems: [
    { category: 'assets', rel_path: 'a', expected_sha: 'x', url: null, status: 'corrupt' },
    { category: 'assets', rel_path: 'b', expected_sha: 'y', url: null, status: 'missing' },
  ],
};

describe('integrity composable', () => {
  beforeEach(() => vi.clearAllMocks());

  it('starts idle', () => {
    const it_ = createIntegrity(
      () => 'i',
      () => false,
    );
    expect(it_.state).toBe('idle');
    it_.dispose();
  });

  it('verify → healthy report sets state to report and problemCount 0', async () => {
    (commands.verifyInstance as any).mockResolvedValue({ status: 'ok', data: healthy });
    const it_ = createIntegrity(
      () => 'i',
      () => false,
    );
    await it_.verify();
    expect(it_.state).toBe('report');
    expect(it_.report?.healthy).toBe(true);
    expect(it_.problemCount).toBe(0);
    it_.dispose();
  });

  it('verify → broken report exposes problemCount', async () => {
    (commands.verifyInstance as any).mockResolvedValue({ status: 'ok', data: broken });
    const it_ = createIntegrity(
      () => 'i',
      () => false,
    );
    await it_.verify();
    expect(it_.problemCount).toBe(2);
    it_.dispose();
  });

  it('does not verify while the game is running', async () => {
    const it_ = createIntegrity(
      () => 'i',
      () => true,
    );
    await it_.verify();
    expect(commands.verifyInstance).not.toHaveBeenCalled();
    expect(it_.state).toBe('idle');
    it_.dispose();
  });

  it('repair sets state back to report with post-repair data', async () => {
    (commands.verifyInstance as any).mockResolvedValue({ status: 'ok', data: broken });
    (commands.repairInstance as any).mockResolvedValue({ status: 'ok', data: healthy });
    const it_ = createIntegrity(
      () => 'i',
      () => false,
    );
    await it_.verify();
    await it_.repair();
    expect(commands.repairInstance).toHaveBeenCalledWith('i');
    expect(it_.report?.healthy).toBe(true);
    it_.dispose();
  });
});
