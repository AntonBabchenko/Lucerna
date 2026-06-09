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

function buttonByName(buttons: HTMLElement[], re: RegExp): HTMLElement {
  const btn = buttons.find((b) => re.test(b.textContent ?? ''));
  if (!btn) throw new Error(`no button matching ${re}`);
  return btn;
}

describe('BulkActionBar busy state', () => {
  it('spins ONLY the in-flight action button, not the others', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true, busyAction: 'update' } as never,
    });
    const buttons = getAllByRole('button');
    // The clicked action spins...
    const update = buttonByName(buttons, /update/i);
    expect(update.querySelector('[role="status"]')).not.toBeNull();
    // ...while the sibling actions are disabled but do NOT spin.
    const spinning = buttons.filter((b) => b.querySelector('[role="status"]'));
    expect(spinning).toEqual([update]);
    // All action buttons are disabled while any op runs (aggregate `busy`).
    for (const name of [/enable/i, /disable/i, /update/i, /uninstall/i]) {
      expect(buttonByName(buttons, name).hasAttribute('disabled')).toBe(true);
    }
  });

  it('disables all actions but spins none when busy with no specific action', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true, busyAction: null } as never,
    });
    const buttons = getAllByRole('button');
    expect(buttons.filter((b) => b.querySelector('[role="status"]'))).toEqual([]);
    expect(buttonByName(buttons, /enable/i).hasAttribute('disabled')).toBe(true);
  });

  it('spins the matching button for each action', () => {
    for (const [action, re] of [
      ['enable', /enable/i],
      ['disable', /disable/i],
      ['uninstall', /uninstall/i],
    ] as const) {
      const { getAllByRole, unmount } = render(BulkActionBar, {
        props: { ...baseProps, busy: true, busyAction: action } as never,
      });
      const buttons = getAllByRole('button');
      expect(buttonByName(buttons, re).querySelector('[role="status"]')).not.toBeNull();
      unmount();
    }
  });

  it('Clear button never spins and stays enabled while busy', () => {
    const { getByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true, busyAction: 'update' } as never,
    });
    const clear = getByRole('button', { name: /clear/i });
    expect(clear.querySelector('[role="status"]')).toBeNull();
    expect(clear.hasAttribute('disabled')).toBe(false);
  });

  it('no spinners and actions enabled when idle', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: false, busyAction: null } as never,
    });
    const buttons = getAllByRole('button');
    expect(buttons.filter((b) => b.querySelector('[role="status"]'))).toEqual([]);
    expect(buttonByName(buttons, /enable/i).hasAttribute('disabled')).toBe(false);
  });
});
