// InstalledToolbar is presentational — callback props + in-flight flags. «Check for updates» spins on
// its own flag; «Update all» only opens the review (T26); the re-check and the mods folder live in
// the ⋯ menu (spec D7), with a labelled spinner while the re-check runs.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledToolbar from '$lib/mods/installed/InstalledToolbar.svelte';

const base = () => ({
  counts: {
    total: 3,
    enabled: 2,
    disabled: 1,
    updates: 1,
    issues: 2,
    needed: 0,
    unusedLibraries: 0,
  },
  filter: '',
  sortBy: 'name-asc' as const,
  viewFilter: 'all' as const,
  busy: false,
  checking: false,
  updateCount: 1,
  checkedAtMs: null as number | null,
  rechecking: false,
  onCheckUpdates: vi.fn(),
  onUpdateAll: vi.fn(),
  onRecheckAll: vi.fn(),
  onOpenModsFolder: vi.fn(),
});

const spinnerIn = (btn: HTMLElement | null) => btn?.querySelector('[role="status"]') ?? null;
const more = () => screen.getByRole('button', { name: /more actions/i });

describe('InstalledToolbar', () => {
  it('Check updates shows a spinner while checking, and none at rest', async () => {
    const { rerender } = render(InstalledToolbar, { props: base() });
    expect(spinnerIn(screen.getByRole('button', { name: /check.*updates|checking/i }))).toBeNull();
    await rerender({ ...base(), checking: true });
    expect(
      spinnerIn(screen.getByRole('button', { name: /check.*updates|checking/i })),
    ).not.toBeNull();
  });

  it('Update all only opens the review: it never spins, and is off while another action runs', async () => {
    const p = base();
    const { rerender } = render(InstalledToolbar, { props: p });
    const btn = () => screen.getByRole('button', { name: /update all/i }) as HTMLButtonElement;
    await fireEvent.click(btn());
    expect(p.onUpdateAll).toHaveBeenCalledOnce();
    await rerender({ ...base(), busy: true });
    expect(btn().disabled).toBe(true);
    expect(spinnerIn(btn())).toBeNull();
  });

  it('says when the updates were last checked', () => {
    render(InstalledToolbar, { props: { ...base(), checkedAtMs: Date.now() - 2 * 86_400_000 } });
    expect(screen.getByTestId('updates-checked-at').textContent).toMatch(/checked 2d ago/);
  });

  it('says nothing about a check while one runs, or when none ran', async () => {
    const { rerender } = render(InstalledToolbar, { props: base() });
    expect(screen.queryByTestId('updates-checked-at')).toBeNull();
    await rerender({ ...base(), checkedAtMs: Date.now(), checking: true });
    expect(screen.queryByTestId('updates-checked-at')).toBeNull();
  });

  it('the ⋯ menu re-checks compatibility and dependencies, and opens the mods folder', async () => {
    const p = base();
    render(InstalledToolbar, { props: p });
    await fireEvent.click(more());
    expect(screen.getAllByRole('menuitem').map((m) => m.textContent?.trim())).toEqual([
      'Re-check compatibility and dependencies',
      'Open mods folder',
    ]);
    await fireEvent.click(screen.getByRole('menuitem', { name: /re-check/i }));
    expect(p.onRecheckAll).toHaveBeenCalledOnce();
    await fireEvent.click(more());
    await fireEvent.click(screen.getByRole('menuitem', { name: /open mods folder/i }));
    expect(p.onOpenModsFolder).toHaveBeenCalledOnce();
  });

  it('while the re-check runs, a labelled spinner shows and the item is off', async () => {
    render(InstalledToolbar, { props: { ...base(), rechecking: true } });
    expect(
      screen.getByRole('status', { name: /checking compatibility and dependencies/i }),
    ).toBeTruthy();
    await fireEvent.click(more());
    expect(
      (screen.getByRole('menuitem', { name: /re-check/i }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it('carries no compatibility / dependency buttons and no counts line', () => {
    render(InstalledToolbar, { props: base() });
    expect(screen.queryByRole('button', { name: /check compatibility|re-check deps/i })).toBeNull();
    expect(screen.queryByText(/^Total:/)).toBeNull();
  });

  it('stays on screen, with its chips, while the list scrolls', () => {
    render(InstalledToolbar, { props: base() });
    const bar = screen.getByTestId('installed-toolbar');
    expect(bar.className).toMatch(/\bsticky\b/);
    expect(bar.className).toMatch(/\btop-0\b/);
    // Above the rows: they are positioned (the card's accent strip) and would otherwise paint
    // over the bar as they scroll under it.
    expect(bar.className).toMatch(/\bz-10\b/);
    expect(bar.className).toContain('bg-base');
    expect(bar.contains(screen.getByRole('radiogroup'))).toBe(true);
  });
});

// Plan §5d (from the #464 merge): the bar's z-10 makes it a stacking context, so its ⋯ menu and the
// menu's click-scrim sat at 10 too — under the sticky pager after the list, which painted over the
// menu and took the click meant to close it, changing the page. While a popover of the bar is open
// its trigger says so (`aria-expanded`), and the bar lifts itself to the popover tier. Only a
// browser paints this: these pin the class the lift hangs on and that its selector holds exactly
// while something is open.
describe('InstalledToolbar with a menu or list open', () => {
  const LIFT = 'has-[[aria-expanded=true]]:z-[var(--z-popover)]';
  // What `:has([aria-expanded=true])` asks. Not `bar.matches(':has(…)')`: happy-dom keeps its
  // first answer for a selector after the attribute under it changes.
  const lifted = (bar: HTMLElement) => bar.querySelector('[aria-expanded="true"]') !== null;

  it('lifts itself above the pager and the list while its ⋯ menu is open', async () => {
    render(InstalledToolbar, { props: base() });
    const bar = screen.getByTestId('installed-toolbar');
    expect(bar.classList).toContain('z-10');
    expect(bar.classList).toContain(LIFT);
    expect(lifted(bar)).toBe(false);
    await fireEvent.click(more());
    const menu = screen.getByRole('menu');
    expect(lifted(bar)).toBe(true);
    // The menu and its scrim are the bar's: the lift carries them.
    expect(bar.contains(menu)).toBe(true);
    await fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(lifted(bar)).toBe(false);
  });

  it('lifts itself while its sort list is open too', async () => {
    render(InstalledToolbar, { props: base() });
    const bar = screen.getByTestId('installed-toolbar');
    await fireEvent.click(screen.getByRole('combobox', { name: 'Sort:' }));
    expect(bar.contains(screen.getByRole('listbox'))).toBe(true);
    expect(lifted(bar)).toBe(true);
  });
});
