import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (fn: (v: (k: string) => string) => void) => {
      fn((k) => k);
      return () => {};
    },
  },
}));

import RepairConfirmCard from '$lib/logs/RepairConfirmCard.svelte';

describe('RepairConfirmCard', () => {
  it('raise_heap: confirm emits a raise_heap choice', async () => {
    const onConfirm = vi.fn();
    render(RepairConfirmCard, {
      props: {
        plan: { kind: 'raise_heap', from_mb: 2048, to_mb: 4096 },
        onConfirm,
        onCancel: vi.fn(),
      },
    });
    await fireEvent.click(screen.getByTestId('repair-confirm'));
    expect(onConfirm).toHaveBeenCalledWith({ kind: 'raise_heap', to_mb: 4096 });
  });

  it('resolve_conflict: Confirm is disabled until a candidate is chosen', async () => {
    const onConfirm = vi.fn();
    render(RepairConfirmCard, {
      props: {
        plan: {
          kind: 'resolve_conflict',
          candidates: [
            { sha1: 'aaa', name: 'Sodium', compat_flagged: false, swap_target: null, swap_version_label: null },
            { sha1: 'bbb', name: 'Old Lib', compat_flagged: true, swap_target: null, swap_version_label: null },
          ],
        },
        onConfirm,
        onCancel: vi.fn(),
      },
    });
    const confirm = screen.getByTestId('repair-confirm') as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    await fireEvent.click(screen.getByTestId('conflict-disable-bbb'));
    expect(confirm.disabled).toBe(false);
    await fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith({ kind: 'disable_mod', sha1: 'bbb' });
  });
});
