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
  categories: [
    { category: 'client', total: 5, ok: 3, missing: 1, corrupt: 1, built_differently: 0 },
  ],
  problem_count: 2,
};

// Forge files an older Lucerna built with its own code (spec 2026-10-16
// §5.5): they differ from Forge's declared bytes but are not damaged.
const builtDifferentlyStatus: IntegrityStatus = {
  healthy: false,
  checked_unix_ms: null,
  categories: [
    { category: 'libraries', total: 40, ok: 37, missing: 0, corrupt: 0, built_differently: 3 },
  ],
  problem_count: 3,
};

describe('IntegritySection', () => {
  beforeEach(() => {
    __resetTasksForTest();
  });

  it('says Forge files built differently are not damaged, and that Repair rebuilds them', () => {
    const { container, getByRole, getByTestId } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst', status: builtDifferentlyStatus },
    });
    const text = container.textContent ?? '';
    expect(text).not.toMatch(/corrupt|damaged/i);
    expect(text).toMatch(/3 built differently from Forge's own installer/);
    const note = getByTestId('integrity-built-differently').textContent ?? '';
    expect(note).toMatch(/If the game started with them before, it still does/);
    expect(note).toMatch(/Repair rebuilds them the way Forge's installer does/);
    expect(getByRole('button', { name: 'Repair (3)' })).toBeTruthy();
  });

  it('keeps real damage and files built differently apart in one row', () => {
    const { container } = render(IntegritySection, {
      props: {
        instanceId: 'inst-1',
        name: 'Inst',
        status: {
          ...builtDifferentlyStatus,
          categories: [
            {
              category: 'libraries',
              total: 40,
              ok: 37,
              missing: 1,
              corrupt: 0,
              built_differently: 2,
            },
          ],
        },
      },
    });
    const text = container.textContent ?? '';
    expect(text).toMatch(/0 corrupt, 1 missing/);
    expect(text).toMatch(/2 built differently from Forge's own installer/);
  });

  it('has no such note when nothing was built differently', () => {
    const { queryByTestId } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst', status: brokenStatus },
    });
    expect(queryByTestId('integrity-built-differently')).toBeNull();
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

  // A Java the user chose is not Lucerna's to check (spec 2026-10-11
  // custom-java §7): the section says it is checked at launch instead of
  // passing it over in silence.
  it("says a Java of the instance's own is checked at every launch", () => {
    const { getByTestId } = render(IntegritySection, {
      props: {
        instanceId: 'inst-1',
        name: 'Inst',
        java: { kind: 'custom', path: '/opt/jdk-21/bin/java' },
      },
    });
    expect(getByTestId('integrity-own-java').textContent).toMatch(/checked at every launch/);
  });

  it("says nothing of the kind for Lucerna's Java", () => {
    const { queryByTestId } = render(IntegritySection, {
      props: { instanceId: 'inst-1', name: 'Inst', java: { kind: 'bundled' } },
    });
    expect(queryByTestId('integrity-own-java')).toBeNull();
  });
});
