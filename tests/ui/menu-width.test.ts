import { fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ContextMenu from '$lib/ui/cards/ContextMenu.svelte';
import type { ContextMenuItem } from '$lib/ui/menu-item';
import OverflowMenu from '$lib/ui/OverflowMenu.svelte';

// Plan §5b V2 (screenshot 02): «Перепроверить совместимость и зависимости» wrapped over three lines
// of the 230 px ⋯ menu and squeezed its icon to ~9 px — and the same fixed width wrapped every long
// item of every menu (the profile menu's «Создать ярлык на рабочем столе…»). A menu is as wide as
// its longest item, never narrower than its minimum; its icons never shrink; it stays on screen,
// the ⋯ menu growing leftwards from under its button.

const LONG = 'Re-check compatibility and dependencies';
const items = (): ContextMenuItem[] => [
  { label: LONG, icon: 'refresh', onSelect: vi.fn() },
  {
    label: 'Open mods folder',
    icon: 'folderOpen',
    disabled: true,
    disabledReason: 'A reason long enough that it must wrap under its label, never widen the menu.',
    onSelect: vi.fn(),
  },
];

/** happy-dom lays nothing out: the menu surface measures `w`, everything else 0. */
function menuMeasures(w: number) {
  return vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.getAttribute('role') === 'menu' ? w : 0;
  });
}

afterEach(() => vi.restoreAllMocks());

describe('Menu width', () => {
  it('is as wide as its longest item, never narrower than its minimum; its icons never shrink', async () => {
    render(OverflowMenu, { props: { items: items(), ariaLabel: 'More' } });
    await fireEvent.click(screen.getByRole('button', { name: 'More' }));
    const menu = screen.getByRole('menu');
    expect(menu.className).toContain('w-max');
    expect(menu.style.minWidth).toBe('230px');
    for (const svg of menu.querySelectorAll('svg')) expect(svg.classList).toContain('shrink-0');
    // A disabled item's reason wraps at the width the labels set; it never sets it.
    const reason = screen.getByText(/A reason long enough/);
    expect(reason.className).toContain('w-0');
    expect(reason.className).toContain('min-w-full');
  });

  it('the ⋯ menu grows leftwards: its right edge stays under its button', async () => {
    menuMeasures(340);
    render(OverflowMenu, { props: { items: items(), ariaLabel: 'More' } });
    const trigger = screen.getByRole('button', { name: 'More' });
    trigger.getBoundingClientRect = () => ({ right: 800, bottom: 40 }) as DOMRect;
    await fireEvent.click(trigger);
    expect(screen.getByRole('menu').style.left).toBe('460px');
  });

  it('a context menu wider than its minimum still stays on screen', async () => {
    menuMeasures(340);
    const trigger = createRawSnippet(() => ({
      render: () => `<button data-testid="trigger">row</button>`,
    }));
    render(ContextMenu, { props: { items: items(), ariaLabel: 'Row', children: trigger } });
    await fireEvent.contextMenu(screen.getByTestId('trigger'), {
      clientX: window.innerWidth - 100,
      clientY: 10,
    });
    expect(screen.getByRole('menu').style.left).toBe(`${window.innerWidth - 340 - 8}px`);
  });
});
