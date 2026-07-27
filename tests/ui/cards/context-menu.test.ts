import { fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import ContextMenu from '$lib/ui/cards/ContextMenu.svelte';

const trigger = createRawSnippet(() => ({
  render: () => `<button data-testid="trigger">card</button>`,
}));

function items(onSelect = vi.fn()) {
  return [
    { label: 'Disable', icon: 'power' as const, onSelect },
    {
      label: 'Uninstall',
      icon: 'trash' as const,
      danger: true,
      separatorBefore: true,
      onSelect: () => {},
    },
  ];
}

describe('ContextMenu', () => {
  it('is closed until a right-click', () => {
    render(ContextMenu, { props: { items: items(), ariaLabel: 'Mod actions', children: trigger } });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('opens on contextmenu and shows the items', async () => {
    render(ContextMenu, { props: { items: items(), ariaLabel: 'Mod actions', children: trigger } });
    await fireEvent.contextMenu(screen.getByTestId('trigger'));
    expect(screen.getByRole('menu')).toBeTruthy();
    expect(screen.getByRole('menuitem', { name: /disable/i })).toBeTruthy();
  });

  it('invokes onSelect and closes when an item is clicked', async () => {
    const onSelect = vi.fn();
    render(ContextMenu, {
      props: { items: items(onSelect), ariaLabel: 'Mod actions', children: trigger },
    });
    await fireEvent.contextMenu(screen.getByTestId('trigger'));
    await fireEvent.click(screen.getByRole('menuitem', { name: /disable/i }));
    expect(onSelect).toHaveBeenCalledOnce();
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('closes on Escape', async () => {
    render(ContextMenu, { props: { items: items(), ariaLabel: 'Mod actions', children: trigger } });
    await fireEvent.contextMenu(screen.getByTestId('trigger'));
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('right-click open pre-highlights nothing; ArrowDown lands on the first item', async () => {
    render(ContextMenu, { props: { items: items(), ariaLabel: 'Mod actions', children: trigger } });
    await fireEvent.contextMenu(screen.getByTestId('trigger'));
    const menuitems = screen.getAllByRole('menuitem');
    expect(menuitems.filter((m) => m.classList.contains('bg-subtle'))).toEqual([]);
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowDown' });
    expect(menuitems[0].classList.contains('bg-subtle')).toBe(true);
  });

  it('Shift+F10 open pre-highlights the first item', async () => {
    render(ContextMenu, { props: { items: items(), ariaLabel: 'Mod actions', children: trigger } });
    const target = screen.getByTestId('trigger');
    target.focus();
    await fireEvent.keyDown(target, { key: 'F10', shiftKey: true });
    const menuitems = screen.getAllByRole('menuitem');
    expect(menuitems[0].classList.contains('bg-subtle')).toBe(true);
  });
});
