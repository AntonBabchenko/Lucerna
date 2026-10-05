// The shared bulk bar: one spinner (the running action), every action disabled while anything
// runs, Clear never gated, and a disabled action that says why.
import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import BulkActionBar, { type BulkBarAction } from '$lib/ui/BulkActionBar.svelte';

const actions: BulkBarAction[] = [
  { id: 'enable', label: 'Enable' },
  { id: 'disable', label: 'Disable' },
  { id: 'update', label: 'Update' },
  { id: 'uninstall', label: 'Remove', intent: 'danger' },
];

const baseProps = {
  allSelected: false,
  selectedCount: 3,
  indeterminate: true,
  hint: 'pick some',
  actions,
  onToggleAll: () => {},
  onAction: () => {},
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
      props: { ...baseProps, busy: true, busyAction: 'update' },
    });
    const buttons = getAllByRole('button');
    const update = buttonByName(buttons, /update/i);
    expect(update.querySelector('[role="status"]')).not.toBeNull();
    const spinning = buttons.filter((b) => b.querySelector('[role="status"]'));
    expect(spinning).toEqual([update]);
    for (const name of [/enable/i, /disable/i, /update/i, /remove/i]) {
      expect(buttonByName(buttons, name).hasAttribute('disabled')).toBe(true);
    }
  });

  it('disables all actions but spins none when busy with no specific action', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true, busyAction: null },
    });
    const buttons = getAllByRole('button');
    expect(buttons.filter((b) => b.querySelector('[role="status"]'))).toEqual([]);
    expect(buttonByName(buttons, /enable/i).hasAttribute('disabled')).toBe(true);
  });

  it('spins the matching button for each action', () => {
    for (const [action, re] of [
      ['enable', /enable/i],
      ['disable', /disable/i],
      ['uninstall', /remove/i],
    ] as const) {
      const { getAllByRole, unmount } = render(BulkActionBar, {
        props: { ...baseProps, busy: true, busyAction: action },
      });
      const buttons = getAllByRole('button');
      expect(buttonByName(buttons, re).querySelector('[role="status"]')).not.toBeNull();
      unmount();
    }
  });

  it('Clear button never spins and stays enabled while busy', () => {
    const { getByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: true, busyAction: 'update' },
    });
    const clear = getByRole('button', { name: /clear/i });
    expect(clear.querySelector('[role="status"]')).toBeNull();
    expect(clear.hasAttribute('disabled')).toBe(false);
  });

  it('no spinners and actions enabled when idle', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: { ...baseProps, busy: false, busyAction: null },
    });
    const buttons = getAllByRole('button');
    expect(buttons.filter((b) => b.querySelector('[role="status"]'))).toEqual([]);
    expect(buttonByName(buttons, /enable/i).hasAttribute('disabled')).toBe(false);
  });

  it('a disabled action is off and keeps a tab stop on its wrapper for the reason', () => {
    const { getAllByRole } = render(BulkActionBar, {
      props: {
        ...baseProps,
        busy: false,
        busyAction: null,
        actions: [{ id: 'update', label: 'Update', disabled: true, disabledReason: 'Check first' }],
      },
    });
    const update = buttonByName(getAllByRole('button'), /update/i);
    expect(update.hasAttribute('disabled')).toBe(true);
    expect((update.parentElement as HTMLElement).getAttribute('tabindex')).toBe('0');
  });

  it('the hint shows only while nothing is selected, and so does nothing else', () => {
    const idle = render(BulkActionBar, {
      props: { ...baseProps, selectedCount: 0, busy: false, busyAction: null },
    });
    expect(idle.getByText('pick some')).toBeTruthy();
    expect(idle.queryByTestId('bulk-bar')).toBeNull();
    idle.unmount();
    const some = render(BulkActionBar, { props: { ...baseProps, busy: false, busyAction: null } });
    expect(some.queryByText('pick some')).toBeNull();
    expect(some.getByText(/3 selected/)).toBeTruthy();
    expect(some.getByTestId('bulk-bar')).toBeTruthy();
  });
});
