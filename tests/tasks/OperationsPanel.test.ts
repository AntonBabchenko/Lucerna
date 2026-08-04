import { fireEvent, render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import OperationsPanel from '$lib/tasks/OperationsPanel.svelte';
import { __resetTasksForTest, finish, start } from '$lib/tasks/registry.svelte';

const base = { scope: {}, phase: null, progress: null };

describe('OperationsPanel', () => {
  beforeEach(() => {
    __resetTasksForTest();
  });

  it('orders sections Running, then Queued, then Finished', () => {
    // 'run' is the first `serial` task, so it starts running immediately
    // (registry.svelte.ts's `start()`); 'q1'/'q2' are serial too, so THEY
    // queue behind it — a `concurrent` first task would never block a
    // later serial one and both would land in Running instead.
    start({ ...base, id: 'run', kind: 'mod-install', lane: 'serial', title: 'Running one' });
    start({ ...base, id: 'q1', kind: 'verify', lane: 'serial', title: 'Queue A' });
    start({ ...base, id: 'q2', kind: 'repair', lane: 'serial', title: 'Queue B' });
    start({ ...base, id: 'fin', kind: 'clone', lane: 'serial', title: 'Finished one' });
    finish('fin', { state: 'ok' });

    const { container } = render(OperationsPanel, { props: { onClose: vi.fn() } });

    const order = Array.from(container.querySelectorAll<HTMLElement>('[data-testid]'))
      .map((el) => el.dataset.testid)
      .filter(
        (id): id is string =>
          !!id &&
          (id.startsWith('operations-panel-section-') || id.startsWith('operations-panel-row-')),
      );

    expect(order).toEqual([
      'operations-panel-section-running',
      'operations-panel-row-run',
      'operations-panel-section-queued',
      'operations-panel-row-q1',
      'operations-panel-row-q2',
      'operations-panel-section-finished',
      'operations-panel-row-fin',
    ]);
  });

  it('lists finished tasks newest-first', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    start({ ...base, id: 'first', kind: 'verify', lane: 'serial', title: 'First' });
    finish('first', { state: 'ok' });
    vi.setSystemTime(2000);
    start({ ...base, id: 'second', kind: 'repair', lane: 'serial', title: 'Second' });
    finish('second', { state: 'ok' });
    vi.useRealTimers();

    const { container } = render(OperationsPanel, { props: { onClose: vi.fn() } });
    const rows = Array.from(
      container.querySelectorAll<HTMLElement>('[data-testid^="operations-panel-row-"]'),
    );
    expect(rows.map((r) => r.dataset.testid)).toEqual([
      'operations-panel-row-second',
      'operations-panel-row-first',
    ]);
  });

  it('offers cancel for a queued task but not for a running one', () => {
    // Both `serial` so the second one genuinely queues behind the first —
    // see the note in the "orders sections" test above.
    start({ ...base, id: 'run', kind: 'mod-install', lane: 'serial', title: 'Running' });
    start({ ...base, id: 'q', kind: 'verify', lane: 'serial', title: 'Queued' });

    const { queryByTestId } = render(OperationsPanel, { props: { onClose: vi.fn() } });

    expect(queryByTestId('operations-panel-cancel-q')).toBeTruthy();
    expect(queryByTestId('operations-panel-cancel-run')).toBeNull();
  });

  it('gives a modal-lane task a lock affordance and no controls at all', () => {
    start({ ...base, id: 'm', kind: 'data-migration', lane: 'modal', title: 'Migrating' });

    const { queryByTestId, getByTestId } = render(OperationsPanel, { props: { onClose: vi.fn() } });

    expect(getByTestId('operations-panel-lock-m')).toBeTruthy();
    expect(queryByTestId('operations-panel-cancel-m')).toBeNull();
    expect(queryByTestId('operations-panel-move-up-m')).toBeNull();
    expect(queryByTestId('operations-panel-move-down-m')).toBeNull();
  });

  it('shows Details only when the task carries a detail report', () => {
    start({ ...base, id: 'withReport', kind: 'mod-install', lane: 'concurrent', title: 'A' });
    finish('withReport', { state: 'ok', details: [{ name: 'a.jar' } as never] });
    start({ ...base, id: 'noReport', kind: 'verify', lane: 'serial', title: 'B' });
    finish('noReport', { state: 'ok' });

    const { queryByTestId } = render(OperationsPanel, { props: { onClose: vi.fn() } });

    expect(queryByTestId('operations-panel-details-withReport')).toBeTruthy();
    expect(queryByTestId('operations-panel-details-noReport')).toBeNull();
  });

  it('calls onDetails with the task when Details is clicked', async () => {
    start({ ...base, id: 'withReport', kind: 'mod-install', lane: 'concurrent', title: 'A' });
    finish('withReport', { state: 'ok', details: [{ name: 'a.jar' } as never] });
    const onDetails = vi.fn();

    const { getByTestId } = render(OperationsPanel, { props: { onClose: vi.fn(), onDetails } });
    await fireEvent.click(getByTestId('operations-panel-details-withReport'));

    expect(onDetails).toHaveBeenCalledTimes(1);
    expect(onDetails.mock.calls[0][0].id).toBe('withReport');
  });

  it('empties the Finished section when Clear is clicked', async () => {
    start({ ...base, id: 'fin', kind: 'verify', lane: 'serial', title: 'Done' });
    finish('fin', { state: 'ok' });

    const { getByTestId, queryByTestId } = render(OperationsPanel, { props: { onClose: vi.fn() } });
    expect(getByTestId('operations-panel-row-fin')).toBeTruthy();

    await fireEvent.click(getByTestId('operations-panel-clear-finished'));

    expect(queryByTestId('operations-panel-row-fin')).toBeNull();
    expect(queryByTestId('operations-panel-section-finished')).toBeNull();
  });

  it('closes on Escape', async () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial', title: 'A' });
    const onClose = vi.fn();
    render(OperationsPanel, { props: { onClose } });

    await fireEvent.keyDown(window, { key: 'Escape' });

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('does not close on scroll — unlike Select/Menu/HelpPopover', async () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial', title: 'A' });
    const onClose = vi.fn();
    render(OperationsPanel, { props: { onClose } });

    await fireEvent.scroll(window);

    expect(onClose).not.toHaveBeenCalled();
  });
});
