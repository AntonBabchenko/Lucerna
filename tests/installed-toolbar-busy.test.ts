// InstalledToolbar busy spinners. The toolbar is purely presentational —
// callback props + boolean in-flight flags — so we drive each flag directly and
// assert the matching button shows a [role="status"] spinner (from BusyButton /
// Spinner). Each async action spins on its OWN flag and is merely disabled when
// a sibling action runs:
//   - "Check updates"  → own flag `checking`,      sibling-disabled by `busy`
//   - "Check compat"   → own flag `checkingCompat`, sibling-disabled by `busy`
//   - "Update all"     → no flag of its own: it only opens the review, which spins while the
//                        updates run; disabled by `busy`
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
  graphLoading: false,
  updateCount: 1,
  onCheckUpdates: vi.fn(),
  onRecheckDeps: vi.fn(),
  onUpdateAll: vi.fn(),
  checkingCompat: false,
  onCheckCompat: vi.fn(),
});

const spinnerIn = (btn: HTMLElement | null) => btn?.querySelector('[role="status"]') ?? null;

describe('InstalledToolbar — busy spinners on async actions', () => {
  it('Check updates shows a spinner while checking, and none at rest', () => {
    const { rerender } = render(InstalledToolbar, { props: { ...base(), checking: false } });
    expect(spinnerIn(screen.getByRole('button', { name: /check.*updates|checking/i }))).toBeNull();

    rerender({ ...base(), checking: true });
    expect(
      spinnerIn(screen.getByRole('button', { name: /check.*updates|checking/i })),
    ).not.toBeNull();
  });

  it('Check compat shows a spinner while checkingCompat, and none at rest', () => {
    const { rerender } = render(InstalledToolbar, { props: { ...base(), checkingCompat: false } });
    expect(spinnerIn(screen.getByRole('button', { name: /compat/i }))).toBeNull();

    // When only checkingCompat is true, the compat button swaps to "Checking…"
    // (the update-check button still reads "Check for updates"), so "Checking…"
    // uniquely identifies the compat button here.
    rerender({ ...base(), checkingCompat: true });
    expect(spinnerIn(screen.getByRole('button', { name: /checking/i }))).not.toBeNull();
  });

  it('Update all only opens the review: it never spins, and is off while another action runs', async () => {
    const p = base();
    const { rerender } = render(InstalledToolbar, { props: p });
    const btn = () => screen.getByRole('button', { name: /update all/i }) as HTMLButtonElement;
    await fireEvent.click(btn());
    expect(p.onUpdateAll).toHaveBeenCalledOnce();
    rerender({ ...base(), busy: true });
    expect(btn().disabled).toBe(true);
    expect(spinnerIn(btn())).toBeNull();
  });

  it('says when the updates were last checked', () => {
    render(InstalledToolbar, { props: { ...base(), checkedAtMs: Date.now() - 2 * 86_400_000 } });
    expect(screen.getByTestId('updates-checked-at').textContent).toMatch(/checked 2d ago/);
  });

  it('says nothing about a check while one runs, or when none ran', () => {
    const { rerender } = render(InstalledToolbar, { props: base() });
    expect(screen.queryByTestId('updates-checked-at')).toBeNull();
    rerender({ ...base(), checkedAtMs: Date.now(), checking: true });
    expect(screen.queryByTestId('updates-checked-at')).toBeNull();
  });

  it('a button spins only for its own action, not a sibling action', () => {
    // While only `checking` is true, Check-updates spins but Check-compat / Update-all do not.
    render(InstalledToolbar, { props: { ...base(), checking: true } });
    expect(
      spinnerIn(screen.getByRole('button', { name: /check.*updates|checking/i })),
    ).not.toBeNull();
    expect(spinnerIn(screen.getByRole('button', { name: /compat/i }))).toBeNull();
    expect(spinnerIn(screen.getByRole('button', { name: /update all/i }))).toBeNull();
  });

  it('recheck-deps button shows a spinner while graphLoading, and none at rest', () => {
    const { rerender, container } = render(InstalledToolbar, {
      props: { ...base(), graphLoading: false },
    });
    // At rest: recheck-deps button has no spinner
    expect(spinnerIn(screen.getByRole('button', { name: /re-check deps/i }))).toBeNull();

    rerender({ ...base(), graphLoading: true });
    // While loading: button is disabled and contains a role="status" spinner.
    // Query via the disabled recheck button (3rd plain <button> after 2 BusyButtons).
    const recheckBtn = container.querySelector(
      'button[disabled]:not([aria-busy])',
    ) as HTMLElement | null;
    expect(recheckBtn).not.toBeNull();
    expect(spinnerIn(recheckBtn)).not.toBeNull();
  });
});
