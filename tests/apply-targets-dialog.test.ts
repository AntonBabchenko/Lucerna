import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ApplyTargetsDialog from '$lib/l10n/ApplyTargetsDialog.svelte';

const mocks = vi.hoisted(() => ({
  l10nApplyTargets: vi.fn(),
  l10nApply: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));

const target = (over: Record<string, unknown> = {}) => ({
  instanceId: 'b',
  name: 'Instance B',
  covered: true,
  state: 'not_applied',
  appliedOtherLang: null,
  isRunning: false,
  prefillActive: false,
  candidate: true,
  actionable: true,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe('ApplyTargetsDialog', () => {
  it('says so instead of vanishing when the user asked and there is nothing to do', async () => {
    // A press deserves an answer. Self-closing is right only for an offer
    // nobody asked for.
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [target({ actionable: false, isRunning: true })],
    });
    const onClose = vi.fn();
    render(ApplyTargetsDialog, {
      props: { lang: 'ru_ru', exclude: null, unsolicited: false, onClose },
    });

    await waitFor(() => screen.getByTestId('apply-targets-none'));
    expect(onClose).not.toHaveBeenCalled();
  });

  it('still vanishes when it offered itself and there is nothing to do', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [target({ actionable: false, isRunning: true })],
    });
    const onClose = vi.fn();
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose } });

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(screen.queryByTestId('apply-targets-none')).toBeNull();
  });

  it('drops the stale state chip once a row has a result', async () => {
    // The chip is a snapshot from load; the result line is newer. Showing
    // both put "not applied" next to "Applied" on one row.
    mocks.l10nApplyTargets.mockResolvedValue({ status: 'ok', data: [target()] });
    mocks.l10nApply.mockResolvedValue({ status: 'ok', data: true });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-run'));
    const row = () => screen.getByTestId('apply-targets-row-b');
    expect(row().textContent).toContain('not applied');

    await fireEvent.click(screen.getByTestId('apply-targets-run'));
    await waitFor(() => screen.getByTestId('apply-targets-result-b'));
    expect(row().textContent).not.toContain('not applied');
  });

  it('keeps saying not applied for a deferred row, without contradicting itself', async () => {
    // `deferred` means the pack was written but cannot switch on yet, so the
    // pre-action chip was not WRONG — which is why it is hidden rather than
    // flipped to "applied".
    mocks.l10nApplyTargets.mockResolvedValue({ status: 'ok', data: [target()] });
    mocks.l10nApply.mockResolvedValue({ status: 'ok', data: false });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-run'));
    await fireEvent.click(screen.getByTestId('apply-targets-run'));

    await waitFor(() => screen.getByTestId('apply-targets-result-b'));
    const row = screen.getByTestId('apply-targets-row-b');
    expect(row.textContent).not.toContain('not applied');
    expect(screen.getByTestId('apply-targets-result-b').className).toContain('text-accent');
  });

  it('reaches a terminal state instead of staying re-pressable', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({ status: 'ok', data: [target()] });
    mocks.l10nApply.mockResolvedValue({ status: 'ok', data: true });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-run'));
    await fireEvent.click(screen.getByTestId('apply-targets-run'));

    await waitFor(() =>
      expect((screen.getByTestId('apply-targets-run') as HTMLButtonElement).disabled).toBe(true),
    );
    expect((screen.getByTestId('apply-targets-check-b') as HTMLInputElement).checked).toBe(false);
    expect(screen.getByTestId('apply-targets-cancel').textContent?.trim()).toBe('Close');
    expect(mocks.l10nApply).toHaveBeenCalledTimes(1);
  });

  it('leaves a failed row ticked so a second press retries only it', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [target(), target({ instanceId: 'c', name: 'Instance C' })],
    });
    mocks.l10nApply.mockResolvedValueOnce({ status: 'ok', data: true }).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: 'x', message: 'nope' },
    });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-run'));
    await fireEvent.click(screen.getByTestId('apply-targets-run'));

    await waitFor(() => screen.getByTestId('apply-targets-result-c'));
    expect((screen.getByTestId('apply-targets-check-b') as HTMLInputElement).checked).toBe(false);
    expect((screen.getByTestId('apply-targets-check-c') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId('apply-targets-run') as HTMLButtonElement).disabled).toBe(false);
  });

  it('lists candidates, disables busy rows and discloses a language replacement', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [
        target(),
        target({ instanceId: 'r', name: 'Running', isRunning: true, actionable: false }),
        target({ instanceId: 'p', name: 'Prefilling', prefillActive: true, actionable: false }),
        target({ instanceId: 'o', name: 'Other lang', appliedOtherLang: 'de_de' }),
      ],
    });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => expect(screen.getByTestId('apply-targets-row-b')).toBeTruthy());
    expect((screen.getByTestId('apply-targets-check-r') as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId('apply-targets-check-p') as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId('apply-targets-check-b') as HTMLInputElement).disabled).toBe(false);
    // Ticked by default exactly when the row is actionable: a box the run
    // would act on must look ticked, and one it would skip must not.
    expect((screen.getByTestId('apply-targets-check-b') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId('apply-targets-check-r') as HTMLInputElement).checked).toBe(false);
    expect(screen.getByTestId('apply-targets-row-o').textContent).toContain('de_de');
  });

  it('excludes the instance that just applied', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [target({ instanceId: 'a', name: 'Alpha' }), target()],
    });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: 'a', onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-row-b'));
    expect(screen.queryByTestId('apply-targets-row-a')).toBeNull();
  });

  it('closes itself when nothing is actionable instead of showing dead rows', async () => {
    const onClose = vi.fn();
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [target({ isRunning: true, actionable: false })],
    });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose } });

    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('renders a deferred apply as its own outcome, not as success', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({ status: 'ok', data: [target()] });
    mocks.l10nApply.mockResolvedValue({ status: 'ok', data: false });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-run'));
    await fireEvent.click(screen.getByTestId('apply-targets-run'));

    await waitFor(() =>
      expect(screen.getByTestId('apply-targets-result-b').textContent).toMatch(/first launch/i),
    );
    expect(mocks.l10nApply).toHaveBeenCalledWith('b', 'ru_ru');
  });

  it('applies every ticked row and reports each one', async () => {
    mocks.l10nApplyTargets.mockResolvedValue({
      status: 'ok',
      data: [target(), target({ instanceId: 'c', name: 'Instance C' })],
    });
    mocks.l10nApply.mockResolvedValue({ status: 'ok', data: true });
    render(ApplyTargetsDialog, { props: { lang: 'ru_ru', exclude: null, onClose: vi.fn() } });

    await waitFor(() => screen.getByTestId('apply-targets-run'));
    await fireEvent.click(screen.getByTestId('apply-targets-run'));

    await waitFor(() => expect(mocks.l10nApply).toHaveBeenCalledTimes(2));
    expect(screen.getByTestId('apply-targets-result-b').textContent?.length).toBeGreaterThan(0);
    expect(screen.getByTestId('apply-targets-result-c').textContent?.length).toBeGreaterThan(0);
  });
});
