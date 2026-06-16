import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { diagnoseLatest: vi.fn(), buildRepairPlan: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import LogDiagnosisBanner from '$lib/logs/LogDiagnosisBanner.svelte';
import { __resetLogDiagnosisForTest, refreshDiagnosis } from '$lib/logs/log-diagnosis.svelte';

const props = {
  instanceId: 'inst-1',
  instanceName: 'I',
  mcVersion: '1.20.1',
  loader: 'fabric' as const,
  gameRunning: false,
};

afterEach(() => {
  __resetLogDiagnosisForTest();
  vi.clearAllMocks();
});

describe('LogDiagnosisBanner', () => {
  it('renders nothing when status is none', () => {
    const { queryByTestId } = render(LogDiagnosisBanner, { props });
    expect(queryByTestId('log-diagnosis-banner')).toBeNull();
  });

  it('advisory status shows the banner with guidance but no fix button', async () => {
    // disk-full is an advisory pattern (no repair) — the banner must surface the
    // diagnosis + recommendation, but never a Fix button.
    vi.mocked(commands.diagnoseLatest).mockResolvedValue({
      status: 'ok',
      data: {
        status: 'advisory',
        diagnosis: {
          pattern_id: 'disk-full',
          title: 't',
          explanation: 'e',
          recommendation: 'r',
          matched_excerpt: 'x',
          repair: null,
        },
        path: 'p',
        signature: 's',
      },
      // biome-ignore lint/suspicious/noExplicitAny: test fixture for the mocked command result
    } as any);
    await refreshDiagnosis('inst-1');
    const { queryByTestId } = render(LogDiagnosisBanner, { props });
    expect(queryByTestId('log-diagnosis-banner')).not.toBeNull();
    expect(queryByTestId('diagnosis-fix')).toBeNull();
  });
});
