import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { diagnoseLatest: vi.fn(), buildRepairPlan: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import LogDiagnosisBanner from '$lib/logs/LogDiagnosisBanner.svelte';
import { __resetLogDiagnosisForTest, refreshDiagnosis } from '$lib/logs/log-diagnosis.svelte';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

const props = {
  instanceId: 'inst-1',
  instanceName: 'I',
  mcVersion: '1.20.1',
  loader: 'fabric' as const,
  gameRunning: false,
};

// Advisory fixture: a non-actionable diagnosis (banner visible, no fix button).
function mockAdvisory(signature = 's') {
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
      signature,
    },
    // biome-ignore lint/suspicious/noExplicitAny: test fixture for the mocked command result
  } as any);
}

afterEach(() => {
  __resetLogDiagnosisForTest();
  diagnosisDismiss.reset();
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

  it('hides the banner when the dismiss button is clicked', async () => {
    mockAdvisory();
    await refreshDiagnosis('inst-1');
    const { queryByTestId, getByTestId } = render(LogDiagnosisBanner, { props });
    expect(queryByTestId('log-diagnosis-banner')).not.toBeNull();
    await fireEvent.click(getByTestId('log-diagnosis-dismiss'));
    expect(queryByTestId('log-diagnosis-banner')).toBeNull();
  });

  it('stays hidden for the same diagnosis but resurfaces for a different one', async () => {
    mockAdvisory('sig-1');
    await refreshDiagnosis('inst-1');
    const first = render(LogDiagnosisBanner, { props });
    await fireEvent.click(first.getByTestId('log-diagnosis-dismiss'));
    expect(first.queryByTestId('log-diagnosis-banner')).toBeNull();
    first.unmount();

    // Same signature → still hidden on a fresh mount.
    const second = render(LogDiagnosisBanner, { props });
    expect(second.queryByTestId('log-diagnosis-banner')).toBeNull();
    second.unmount();

    // A different problem (different signature) → banner returns.
    mockAdvisory('sig-2');
    await refreshDiagnosis('inst-1');
    const third = render(LogDiagnosisBanner, { props });
    expect(third.queryByTestId('log-diagnosis-banner')).not.toBeNull();
  });

  it('re-shows a dismissed banner once the dismissal is cleared (restore)', async () => {
    mockAdvisory();
    await refreshDiagnosis('inst-1');
    const first = render(LogDiagnosisBanner, { props });
    await fireEvent.click(first.getByTestId('log-diagnosis-dismiss'));
    expect(first.queryByTestId('log-diagnosis-banner')).toBeNull();
    first.unmount();
    // The restore badge lives in LogsPopover; assert the banner returns once the
    // dismissal is cleared (what that badge does on click).
    diagnosisDismiss.restore('log:inst-1');
    const second = render(LogDiagnosisBanner, { props });
    expect(second.queryByTestId('log-diagnosis-banner')).not.toBeNull();
  });
});
