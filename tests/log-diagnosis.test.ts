import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { diagnoseLatest: vi.fn() },
}));

import { commands, type LatestDiagnosis } from '$lib/ipc/bindings';
import {
  __resetLogDiagnosisForTest,
  diagnosisStatus,
  hasDiagnosisIndicator,
  latestDiagnosis,
  refreshDiagnosis,
} from '$lib/logs/log-diagnosis.svelte';

const ok = (d: LatestDiagnosis) => ({ status: 'ok', data: d }) as const;

afterEach(() => {
  __resetLogDiagnosisForTest();
  vi.clearAllMocks();
});

describe('log-diagnosis store', () => {
  it('stores an actionable diagnosis and lights the indicator', async () => {
    vi.mocked(commands.diagnoseLatest).mockResolvedValue(
      ok({ status: 'actionable', diagnosis: null, path: 'p', signature: 's' }),
    );
    await refreshDiagnosis('inst-1');
    expect(diagnosisStatus()).toBe('actionable');
    expect(hasDiagnosisIndicator()).toBe(true);
  });

  it('handled and none do NOT light the indicator', async () => {
    vi.mocked(commands.diagnoseLatest).mockResolvedValue(
      ok({ status: 'handled', diagnosis: null, path: 'p', signature: 's' }),
    );
    await refreshDiagnosis('inst-1');
    expect(hasDiagnosisIndicator()).toBe(false);

    vi.mocked(commands.diagnoseLatest).mockResolvedValue(
      ok({ status: 'none', diagnosis: null, path: null, signature: null }),
    );
    await refreshDiagnosis('inst-1');
    expect(diagnosisStatus()).toBe('none');
    expect(hasDiagnosisIndicator()).toBe(false);
  });

  it('clears when instanceId is null', async () => {
    vi.mocked(commands.diagnoseLatest).mockResolvedValue(
      ok({ status: 'advisory', diagnosis: null, path: 'p', signature: 's' }),
    );
    await refreshDiagnosis('inst-1');
    await refreshDiagnosis(null);
    expect(diagnosisStatus()).toBe('none');
    expect(latestDiagnosis()).toBeNull();
  });

  it('a null-clear invalidates an in-flight fetch (no stale write-back)', async () => {
    let resolveFetch: (v: ReturnType<typeof ok>) => void = () => {};
    vi.mocked(commands.diagnoseLatest).mockReturnValue(
      new Promise<ReturnType<typeof ok>>((r) => {
        resolveFetch = r;
      }),
    );
    const inFlight = refreshDiagnosis('inst-1'); // starts the fetch, not yet resolved
    await refreshDiagnosis(null); // clears + bumps seq while fetch is pending
    resolveFetch(ok({ status: 'actionable', diagnosis: null, path: 'p', signature: 's' }));
    await inFlight; // the now-stale response must NOT overwrite the cleared state
    expect(diagnosisStatus()).toBe('none');
    expect(latestDiagnosis()).toBeNull();
  });
});
