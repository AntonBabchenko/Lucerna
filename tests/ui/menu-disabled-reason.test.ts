import { fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import ContextMenu from '$lib/ui/cards/ContextMenu.svelte';
import type { ContextMenuItem } from '$lib/ui/menu-item';
import { estimateMenuHeight } from '$lib/ui/menu-metrics';

const trigger = createRawSnippet(() => ({
  render: () => `<button data-testid="trigger">row</button>`,
}));

function open(items: ContextMenuItem[]) {
  render(ContextMenu, { props: { items, ariaLabel: 'Row actions', children: trigger } });
  return fireEvent.contextMenu(screen.getByTestId('trigger'));
}

describe('Menu — disabledReason', () => {
  it('renders the reason inside a disabled item and does not activate it', async () => {
    const onSelect = vi.fn();
    await open([
      { label: 'Clone', onSelect: () => {} },
      { label: 'Delete', disabled: true, disabledReason: 'Stop the game first.', onSelect },
    ]);
    const item = screen.getByRole('menuitem', { name: /delete/i }) as HTMLButtonElement;
    expect(item.disabled).toBe(true);
    expect(item.textContent).toContain('Stop the game first.');
    await fireEvent.click(item);
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole('menu')).toBeTruthy();
  });

  it('arrow keys still skip a disabled item that has a reason', async () => {
    await open([
      { label: 'Delete', disabled: true, disabledReason: 'Why', onSelect: () => {} },
      { label: 'Clone', onSelect: () => {} },
    ]);
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowDown' });
    const [del, clone] = screen.getAllByRole('menuitem');
    expect(del.classList.contains('bg-subtle')).toBe(false);
    expect(clone.classList.contains('bg-subtle')).toBe(true);
  });

  it('shows no reason for an enabled item or a reasonless disabled one', async () => {
    await open([
      { label: 'Clone', disabledReason: 'ignored while enabled', onSelect: () => {} },
      { label: 'Delete', disabled: true, onSelect: () => {} },
    ]);
    expect(screen.getByRole('menu').textContent).not.toContain('ignored while enabled');
    expect(screen.getByRole('menuitem', { name: 'Delete' }).textContent?.trim()).toBe('Delete');
  });
});

describe('estimateMenuHeight', () => {
  const item: ContextMenuItem = { label: 'x', onSelect: () => {} };

  it('adds room for separators and reason lines', () => {
    expect(estimateMenuHeight([item, item])).toBe(78);
    expect(estimateMenuHeight([item, { ...item, separatorBefore: true }])).toBe(87);
    expect(estimateMenuHeight([{ ...item, disabled: true, disabledReason: 'why' }])).toBe(76);
    // A reason on an ENABLED item is not rendered, so it takes no room.
    expect(estimateMenuHeight([{ ...item, disabledReason: 'why' }])).toBe(44);
  });
});
