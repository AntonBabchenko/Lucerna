import { fireEvent, render, screen } from '@testing-library/svelte';
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

describe('InstalledToolbar view filter (single mutually-exclusive group)', () => {
  it('renders All/Enabled/Disabled plus Updates/Issues as radios with counts', () => {
    render(InstalledToolbar, { props: base() });
    for (const name of [/All/, /Enabled/, /Disabled/, /Updates/, /Issues/]) {
      expect(screen.getByRole('radio', { name })).toBeTruthy();
    }
    // Default selection is All.
    expect(screen.getByRole('radio', { name: /All/ }).getAttribute('aria-checked')).toBe('true');
  });

  it('selecting a chip checks it and unchecks the others (mutually exclusive)', async () => {
    render(InstalledToolbar, { props: base() });
    await fireEvent.click(screen.getByRole('radio', { name: /Updates/ }));
    expect(screen.getByRole('radio', { name: /Updates/ }).getAttribute('aria-checked')).toBe(
      'true',
    );
    // All the others are now unchecked — no AND-combination.
    for (const name of [/All/, /Enabled/, /Disabled/, /Issues/]) {
      expect(screen.getByRole('radio', { name }).getAttribute('aria-checked')).toBe('false');
    }
  });

  it('exactly one radio is checked at any time', async () => {
    render(InstalledToolbar, { props: { ...base(), viewFilter: 'updates' } });
    await fireEvent.click(screen.getByRole('radio', { name: /Disabled/ }));
    const checked = screen
      .getAllByRole('radio')
      .filter((r) => r.getAttribute('aria-checked') === 'true');
    expect(checked).toHaveLength(1);
    expect(checked[0].textContent).toMatch(/Disabled/);
  });

  it('hides Updates/Issues radios when there are none', () => {
    render(InstalledToolbar, {
      props: {
        ...base(),
        counts: { total: 3, enabled: 3, disabled: 0, updates: 0, issues: 0, incompatible: 0 },
      },
    });
    expect(screen.queryByRole('radio', { name: /Updates/ })).toBeNull();
    expect(screen.queryByRole('radio', { name: /Issues/ })).toBeNull();
    // The state filters remain.
    expect(screen.getByRole('radio', { name: /All/ })).toBeTruthy();
  });
});
