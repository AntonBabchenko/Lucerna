import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { diagnoseLatest: vi.fn(), buildRepairPlan: vi.fn() },
}));

import LogDiagnosisBanner from '$lib/logs/LogDiagnosisBanner.svelte';
import { __resetLogDiagnosisForTest } from '$lib/logs/log-diagnosis.svelte';

afterEach(() => __resetLogDiagnosisForTest());

describe('LogDiagnosisBanner', () => {
  it('renders nothing when status is none', () => {
    const { queryByTestId } = render(LogDiagnosisBanner, {
      props: {
        instanceId: 'i',
        instanceName: 'I',
        mcVersion: '1.20.1',
        loader: 'fabric',
        gameRunning: false,
      },
    });
    expect(queryByTestId('log-diagnosis-banner')).toBeNull();
  });
});
