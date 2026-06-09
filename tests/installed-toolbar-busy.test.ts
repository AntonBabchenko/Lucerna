// InstalledToolbar busy spinners. The toolbar is purely presentational —
// callback props + boolean in-flight flags — so we drive each flag directly and
// assert the matching button shows a [role="status"] spinner (from BusyButton /
// Spinner). Each async action spins on its OWN flag and is merely disabled when
// a sibling action runs:
//   - "Check updates"  → own flag `checking`,      sibling-disabled by `busy`
//   - "Check compat"   → own flag `checkingCompat`, sibling-disabled by `busy`
//   - "Update all"     → own flag `busy`
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledToolbar from '$lib/mods/installed/InstalledToolbar.svelte';

const base = () => ({
  counts: { total: 3, enabled: 2, disabled: 1, updates: 1, issues: 2, incompatible: 0 },
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

  it('Update all shows a spinner while busy, and none at rest', () => {
    const { rerender } = render(InstalledToolbar, { props: { ...base(), busy: false } });
    expect(spinnerIn(screen.getByRole('button', { name: /update all/i }))).toBeNull();

    rerender({ ...base(), busy: true });
    expect(spinnerIn(screen.getByRole('button', { name: /update all/i }))).not.toBeNull();
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
});
