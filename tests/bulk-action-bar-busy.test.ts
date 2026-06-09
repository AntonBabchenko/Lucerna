import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import BulkActionBar from '$lib/mods/installed/BulkActionBar.svelte';

const baseProps = {
  allSelected: false,
  selectedCount: 3,
  indeterminate: false,
  canUpdate: true,
  onToggleAll: () => {},
  onEnable: () => {},
  onDisable: () => {},
  onUpdate: () => {},
  onUninstall: () => {},
  onClear: () => {},
};

describe('BulkActionBar busy state', () => {
  it('shows spinners on the action buttons when busy', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true } as never,
    });
    const spinners = getAllByRole('button').filter((b) => b.querySelector('[role="status"]'));
    expect(spinners.length).toBeGreaterThanOrEqual(4);
  });

  it('Clear button has no spinner even when busy', () => {
    const { getByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true } as never,
    });
    const clear = getByRole('button', { name: /clear/i });
    expect(clear.querySelector('[role="status"]')).toBeNull();
  });
});
