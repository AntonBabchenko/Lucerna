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

  it('keeps the report control keyboard-operable, not a click handler on the row', async () => {
    // The wrong implementation this rules out is an `onclick` on the <li>.
    // It looks identical to a mouse user and is unreachable for a keyboard
    // one: a list item is not focusable and fires no click on Enter/Space.
    //
    // (An earlier version of this test clicked a row's Cancel button and
    // asserted the report did NOT open. That pinned nothing: `capsFor` grants
    // `cancellable` only while a task is `queued`, and `details` only exist
    // after `finish()`, so no row can ever carry both. There is no bubbling
    // to guard against.)
    start({ ...base, id: 'rep', kind: 'game-install', lane: 'concurrent', title: 'Reported' });
    finish('rep', { state: 'ok', details: [{ name: 'a.jar' } as never] });
    const onDetails = vi.fn();

    const { getByTestId } = render(OperationsPanel, { props: { onClose: vi.fn(), onDetails } });
    const control = getByTestId('operations-panel-details-rep');

    expect(control.tagName).toBe('BUTTON');
    expect(control.getAttribute('aria-label')).toBeTruthy();

    control.focus();
    expect(document.activeElement).toBe(control);

    await fireEvent.keyDown(control, { key: 'Enter' });
    await fireEvent.click(control);
    expect(onDetails).toHaveBeenCalledTimes(1);
  });

  it('makes the whole row the report control, not a trailing button', () => {
    start({ ...base, id: 'withReport', kind: 'mod-install', lane: 'concurrent', title: 'A' });
    finish('withReport', { state: 'ok', details: [{ name: 'a.jar' } as never] });

    const { getByTestId } = render(OperationsPanel, { props: { onClose: vi.fn() } });

    const control = getByTestId('operations-panel-details-withReport');
    expect(control.className).toContain('absolute');
    expect(control.className).toContain('inset-0');
    // The row must establish the containing block, or `inset-0` resolves
    // against the scrolling panel and one row's overlay covers all of them.
    expect(getByTestId('operations-panel-row-withReport').className).toContain('relative');
  });

  it('gives a row without a report no clickable affordance', () => {
    start({ ...base, id: 'noReport', kind: 'verify', lane: 'serial', title: 'B' });
    finish('noReport', { state: 'ok' });

    const { getByTestId, queryByTestId } = render(OperationsPanel, { props: { onClose: vi.fn() } });

    expect(queryByTestId('operations-panel-details-noReport')).toBeNull();
    expect(queryByTestId('operations-panel-chevron-noReport')).toBeNull();
    expect(getByTestId('operations-panel-row-noReport').className).not.toContain('cursor-pointer');
  });
});
