import { fireEvent, render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import OperationsBar from '$lib/tasks/OperationsBar.svelte';
import { __resetTasksForTest, finish, start, upsertProgress } from '$lib/tasks/registry.svelte';

// `phase`/`progress`/`scope` are always present on StartInput; most cases
// here don't care about `phase`, so a shared base keeps each `start()` call
// focused on the field the test actually exercises.
const base = { scope: {}, phase: null, progress: null };

describe('OperationsBar', () => {
  beforeEach(() => {
    __resetTasksForTest();
  });

  it('renders nothing when the session has no tasks', () => {
    const { queryByTestId } = render(OperationsBar);
    expect(queryByTestId('operations-bar')).toBeNull();
  });

  it('shows speed and ETA on a byte phase', () => {
    start({
      ...base,
      id: 'a',
      kind: 'pack-import',
      lane: 'serial',
      title: 'Some Pack',
      progress: { current: 50, total: 100, unit: 'bytes' },
    });
    // Real adapters always set `rate` via the same `upsertProgress` tick
    // that reports byte progress (see pack-import.ts) — mirror that here
    // rather than inventing a `start()`-time rate the registry never sets.
    upsertProgress('a', { rate: { bytesPerSec: 1048576, etaSeconds: 90 } });

    const { getByTestId, queryByTestId } = render(OperationsBar);

    expect(getByTestId('operations-bar-rate').textContent).toBe('1.0 MB/s');
    expect(getByTestId('operations-bar-eta').textContent).toBe('~1:30');
    expect(getByTestId('operations-bar-progress').getAttribute('aria-valuenow')).toBe('50');
    expect(queryByTestId('operations-bar-counter')).toBeNull();
  });

  it('shows no rate on a file phase', () => {
    // Game install lands here deliberately: no byte total at all, so a rate
    // would be dishonest (see rate.ts's canShowRate doc comment).
    start({
      ...base,
      id: 'a',
      kind: 'game-install',
      lane: 'concurrent',
      title: 'My Instance',
      progress: { current: 42, total: 120, unit: 'files' },
    });

    const { getByTestId, queryByTestId } = render(OperationsBar);

    expect(getByTestId('operations-bar-counter').textContent).toContain('42/120');
    expect(getByTestId('operations-bar-counter').textContent).toContain('35%');
    expect(queryByTestId('operations-bar-rate')).toBeNull();
    expect(queryByTestId('operations-bar-eta')).toBeNull();
    expect(queryByTestId('operations-bar-progress')).toBeNull();
  });

  it('shows a pulsing bar with no percent or ETA when the total is unknown', () => {
    start({ ...base, id: 'a', kind: 'clone', lane: 'serial', title: 'Clone target' });

    const { getByTestId, queryByTestId } = render(OperationsBar);

    expect(getByTestId('operations-bar-progress').hasAttribute('aria-valuenow')).toBe(false);
    expect(queryByTestId('operations-bar-rate')).toBeNull();
    expect(queryByTestId('operations-bar-eta')).toBeNull();
    expect(queryByTestId('operations-bar-counter')).toBeNull();
  });

  it('summarises several tasks by the earliest started', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial', title: 'Inst A' });
    vi.setSystemTime(2000);
    start({ ...base, id: 'b', kind: 'mod-install', lane: 'concurrent', title: 'Mod B' });
    vi.useRealTimers();

    const { getByTestId } = render(OperationsBar);

    // 'a' started first, so it — not 'b' — is the one the strip represents.
    expect(getByTestId('operations-bar-count').textContent?.trim()).toBe('2 operations');
    expect(getByTestId('operations-bar-kind').textContent?.trim()).toBe('Verifying instance');
  });

  it('keeps showing the earliest task until IT finishes, not on a later tick', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial', title: 'Inst A' });
    vi.setSystemTime(2000);
    start({ ...base, id: 'b', kind: 'mod-install', lane: 'concurrent', title: 'Mod B' });
    vi.useRealTimers();

    const { getByTestId } = render(OperationsBar);
    expect(getByTestId('operations-bar-kind').textContent?.trim()).toBe('Verifying instance');

    // A progress tick on the LATER task must not flip the displayed task.
    upsertProgress('b', { progress: { current: 1, total: 4, unit: 'files' } });
    expect(getByTestId('operations-bar-kind').textContent?.trim()).toBe('Verifying instance');
  });

  it('exposes an aria-expanded disclosure toggle for the panel a later task builds', async () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial', title: 'Inst' });
    const { getByTestId } = render(OperationsBar);

    const toggle = getByTestId('operations-bar-toggle');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.getAttribute('aria-label')).toBeTruthy();

    await fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
  });

  it('shows the real phase text alongside the kind label on a byte phase', () => {
    start({
      ...base,
      id: 'a',
      kind: 'game-install',
      lane: 'concurrent',
      title: 'My Instance',
      progress: { current: 10, total: 100, unit: 'files' },
    });
    upsertProgress('a', { phase: 'libraries' });

    const { getByTestId } = render(OperationsBar);

    expect(getByTestId('operations-bar-kind').textContent?.trim()).toBe('Installing Minecraft');
    expect(getByTestId('operations-bar-phase').textContent?.trim()).toBe('Downloading libraries');
  });

  it('shows no phase text when the kind has no phase vocabulary', () => {
    start({ ...base, id: 'a', kind: 'clone', lane: 'serial', title: 'Clone target' });

    const { queryByTestId } = render(OperationsBar);

    expect(queryByTestId('operations-bar-phase')).toBeNull();
  });

  it('opens and closes the panel on the toggle, without pushing the strip', async () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial', title: 'Inst' });
    const { getByTestId, queryByTestId } = render(OperationsBar);

    expect(queryByTestId('operations-panel')).toBeNull();

    await fireEvent.click(getByTestId('operations-bar-toggle'));
    expect(getByTestId('operations-panel')).toBeTruthy();

    await fireEvent.click(getByTestId('operations-bar-toggle'));
    expect(queryByTestId('operations-panel')).toBeNull();
  });

  it('keeps rendering — with a finished summary instead of a spinner — once the only task finishes', () => {
    start({ ...base, id: 'a', kind: 'pack-import', lane: 'serial', title: 'Some Pack' });
    finish('a', { state: 'ok' });

    const { getByTestId, queryByTestId } = render(OperationsBar);

    expect(getByTestId('operations-bar')).toBeTruthy();
    expect(queryByTestId('operations-bar-kind')).toBeNull();
    expect(getByTestId('operations-bar-finished-summary').textContent?.trim()).toBe(
      '1 operation finished',
    );
  });

  it('summarises several finished tasks with real Russian-safe plural text (English pinned here)', () => {
    start({ ...base, id: 'a', kind: 'pack-import', lane: 'serial', title: 'Pack A' });
    start({ ...base, id: 'b', kind: 'clone', lane: 'serial', title: 'Clone B' });
    finish('a', { state: 'ok' });
    finish('b', { state: 'failed' });

    const { getByTestId } = render(OperationsBar);

    expect(getByTestId('operations-bar-finished-summary').textContent?.trim()).toBe(
      '2 operations finished',
    );
  });

  it('still renders nothing when the session truly has no tasks at all, finished included', () => {
    const { queryByTestId } = render(OperationsBar);
    expect(queryByTestId('operations-bar')).toBeNull();
    expect(queryByTestId('operations-bar-finished-summary')).toBeNull();
  });
});
