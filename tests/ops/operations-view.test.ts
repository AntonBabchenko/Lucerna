import { fireEvent, render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { QueuedOp, RunningOp } from '$lib/ops/op-queue.svelte';

// ---------------------------------------------------------------------------
// Control what the module-singleton getters return from the test.
// ---------------------------------------------------------------------------
const mockOpRunning = vi.fn();
const mockOpQueue = vi.fn();
const mockCancelQueued = vi.fn();
const mockMoveQueued = vi.fn();

vi.mock('$lib/ops/op-queue.svelte', () => ({
  opRunning: () => mockOpRunning() as RunningOp | null,
  opQueue: () => mockOpQueue() as QueuedOp[],
  cancelQueued: (id: string) => mockCancelQueued(id),
  moveQueued: (id: string, dir: 'up' | 'down') => mockMoveQueued(id, dir),
}));

import OperationsView from '$lib/ops/OperationsView.svelte';

beforeEach(() => {
  locale.set('en');
  mockOpRunning.mockReset();
  mockOpQueue.mockReset();
  mockCancelQueued.mockReset();
  mockMoveQueued.mockReset();
  mockOpRunning.mockReturnValue(null);
  mockOpQueue.mockReturnValue([]);
});

describe('OperationsView', () => {
  // ── Case 1: nothing running, empty queue ─────────────────────────────────
  it('renders nothing when opRunning is null and opQueue is empty', () => {
    const { queryByTestId } = render(OperationsView);
    expect(queryByTestId('operations-view')).toBeNull();
  });

  // ── Case 2: running INTEGRITY verify op ──────────────────────────────────
  it('shows file count for a running verify op', () => {
    const running: RunningOp = {
      op: { id: 'o1', kind: 'verify', instanceId: 'a', name: 'Alpha' },
      progress: { kind: 'verify', filesDone: 3, filesTotal: 10 },
    };
    mockOpRunning.mockReturnValue(running);

    const { getByTestId, container } = render(OperationsView);
    expect(getByTestId('operations-view')).toBeTruthy();
    // file count is rendered as "filesDone/filesTotal"
    expect(container.textContent).toContain('3/10');
    // label includes instance name
    expect(container.textContent).toContain('Alpha');
  });

  // ── Case 3: running IMPORT op with enriching phase ────────────────────────
  it('shows import phase label for a running import op', () => {
    const running: RunningOp = {
      op: {
        id: 'o2',
        kind: 'import',
        name: 'My Pack',
        request: {
          path: '/tmp/p.mrpack',
          selectedShas: [],
          projectId: null,
          source: null,
          versionId: null,
        },
      },
      progress: { kind: 'import', phase: { phase: 'enriching' }, bytes: null },
    };
    mockOpRunning.mockReturnValue(running);

    const { getByTestId, container } = render(OperationsView);
    expect(getByTestId('operations-view')).toBeTruthy();
    // phaseEnriching → "Identifying bundled mods…"
    expect(container.textContent).toContain('Identifying bundled mods');
  });

  // ── Case 4: two queued ops — cancel and move buttons work ─────────────────
  it('lists queued ops and wires up cancel and move buttons', async () => {
    const queue: QueuedOp[] = [
      { id: 'q1', kind: 'verify', instanceId: 'i1', name: 'First' },
      { id: 'q2', kind: 'repair', instanceId: 'i2', name: 'Second' },
    ];
    mockOpQueue.mockReturnValue(queue);

    const { container } = render(OperationsView);

    // Both items are in the DOM
    expect(container.textContent).toContain('First');
    expect(container.textContent).toContain('Second');

    // Cancel button on the first queued op
    const cancelButtons = container.querySelectorAll('[aria-label="Cancel"]');
    expect(cancelButtons.length).toBe(2);
    await fireEvent.click(cancelButtons[0]);
    expect(mockCancelQueued).toHaveBeenCalledWith('q1');

    // Move-down button on the first queued op (enabled at index 0)
    const downButtons = container.querySelectorAll('[aria-label="Move down"]');
    await fireEvent.click(downButtons[0]);
    expect(mockMoveQueued).toHaveBeenCalledWith('q1', 'down');

    // Move-up button on the second queued op (enabled at last index)
    const upButtons = container.querySelectorAll('[aria-label="Move up"]');
    await fireEvent.click(upButtons[1]);
    expect(mockMoveQueued).toHaveBeenCalledWith('q2', 'up');
  });
});
