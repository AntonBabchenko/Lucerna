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
    // Above the rows: they are positioned (the accent strip, the dependency ring) and would
    // otherwise paint over the bar as they scroll under it.
    expect(bar.className).toMatch(/\bz-10\b/);
    expect(bar.className).toContain('bg-base');
    expect(bar.contains(screen.getByRole('radiogroup'))).toBe(true);
  });
});
