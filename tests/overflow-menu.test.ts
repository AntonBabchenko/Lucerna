import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
import OverflowMenu from '$lib/ui/OverflowMenu.svelte';

function items(over: Partial<ContextMenuItem>[] = []): ContextMenuItem[] {
  const base: ContextMenuItem[] = [
    { label: 'Alpha', onSelect: vi.fn() },
    { label: 'Beta', onSelect: vi.fn() },
    { label: 'Gamma', danger: true, separatorBefore: true, onSelect: vi.fn() },
  ];
  return base.map((it, i) => ({ ...it, ...(over[i] ?? {}) }));
}

afterEach(() => vi.clearAllMocks());

describe('OverflowMenu', () => {
  it('is closed initially — no menu in the DOM', () => {
    render(OverflowMenu, { props: { items: items(), ariaLabel: 'More' } });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('opens on trigger click and lists all items in order', async () => {
    render(OverflowMenu, { props: { items: items(), ariaLabel: 'More' } });
    await fireEvent.click(screen.getByRole('button', { name: /more/i }));
    const menuitems = screen.getAllByRole('menuitem');
    expect(menuitems.map((m) => m.textContent?.trim())).toEqual(['Alpha', 'Beta', 'Gamma']);
  });

  it('renders a danger item with text-danger', async () => {
    render(OverflowMenu, { props: { items: items(), ariaLabel: 'More' } });
    await fireEvent.click(screen.getByRole('button', { name: /more/i }));
    const gamma = screen.getByRole('menuitem', { name: 'Gamma' });
    expect(gamma.className).toContain('text-danger');
  });

  it('invokes onSelect and closes when an item is clicked', async () => {
    const its = items();
    render(OverflowMenu, { props: { items: its, ariaLabel: 'More' } });
    await fireEvent.click(screen.getByRole('button', { name: /more/i }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Beta' }));
    expect(its[1].onSelect).toHaveBeenCalledOnce();
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('does not invoke onSelect for a disabled item', async () => {
    const its = items([{}, { disabled: true }]);
    render(OverflowMenu, { props: { items: its, ariaLabel: 'More' } });
    await fireEvent.click(screen.getByRole('button', { name: /more/i }));
    const beta = screen.getByRole('menuitem', { name: 'Beta' }) as HTMLButtonElement;
    expect(beta.disabled).toBe(true);
    await fireEvent.click(beta);
    expect(its[1].onSelect).not.toHaveBeenCalled();
  });

  it('closes on Escape', async () => {
    render(OverflowMenu, { props: { items: items(), ariaLabel: 'More' } });
    await fireEvent.click(screen.getByRole('button', { name: /more/i }));
    const menu = screen.getByRole('menu');
    await fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
  });
});
