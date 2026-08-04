import { render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// IntegritySection only enqueues through this store; the registry is what now
// drives its button-disabling signal (see `$lib/tasks/registry.svelte`'s
// `taskFor`), so only `enqueueIntegrity` needs a stub here.
vi.mock('$lib/ops/op-queue.svelte', () => ({
  enqueueIntegrity: vi.fn(),
}));

import IntegritySection from '$lib/instances/IntegritySection.svelte';
import type { IntegrityStatus } from '$lib/ipc/bindings';
import { __resetTasksForTest, start } from '$lib/tasks/registry.svelte';

const brokenStatus: IntegrityStatus = {
  healthy: false,
  checked_unix_ms: null,
  categories: [{ category: 'client', total: 5, ok: 3, missing: 1, corrupt: 1 }],
  problem_count: 2,
};

describe('IntegritySection', () => {
  beforeEach(() => {
    __resetTasksForTest();
  });

  it('leaves Verify enabled when no task is active for this instance', () => {
    const { getByRole } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst' },
    });
    expect((getByRole('button', { name: 'Verify' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('disables Verify while a registry task is active for this instance', () => {
    start({
      id: 'v1',
      kind: 'verify',
      scope: { instanceId: 'inst-1' },
      title: 'Inst',
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const { getByRole } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst' },
    });
    expect((getByRole('button', { name: 'Verify' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('does not block an unrelated instance', () => {
    start({
      id: 'v1',
      kind: 'verify',
      scope: { instanceId: 'other-instance' },
      title: 'Other',
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const { getByRole } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst' },
    });
    expect((getByRole('button', { name: 'Verify' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('replaces the Repair button with the live progress readout while a task is active', () => {
    // The Repair button only renders inside the "nothing active" branch (see
    // IntegritySection's template) — while a task is active for this
    // instance, it is superseded by the spinner/progress readout below, so it
    // cannot be clicked at all rather than merely being disabled.
    start({
      id: 'r1',
      kind: 'repair',
      scope: { instanceId: 'inst-1' },
      title: 'Inst',
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const { queryByRole, getByText } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst', status: brokenStatus },
    });
    expect(queryByRole('button', { name: /Repair \(/ })).toBeNull();
    expect(getByText('Repairing… 0/0')).toBeTruthy();
  });

  it('leaves Repair enabled when no task is active for this instance', () => {
    const { getByRole } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst', status: brokenStatus },
    });
    expect((getByRole('button', { name: /Repair/ }) as HTMLButtonElement).disabled).toBe(false);
  });
});
