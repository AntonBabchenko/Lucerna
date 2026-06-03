import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledToolbar from '$lib/mods/installed/InstalledToolbar.svelte';

const base = () => ({
  counts: { total: 3, enabled: 2, disabled: 1, updates: 1, issues: 2 },
  filter: '',
  sortBy: 'name-asc' as const,
  enabledFilter: 'all' as const,
  quickFilter: 'all' as const,
  busy: false,
  checking: false,
  graphLoading: false,
  updateCount: 1,
  onCheckUpdates: vi.fn(),
  onRecheckDeps: vi.fn(),
  onUpdateAll: vi.fn(),
});

describe('InstalledToolbar quick-filters', () => {
  it('renders Updates and Issues chips with counts', () => {
    render(InstalledToolbar, { props: base() });
    expect(screen.getByRole('button', { name: /Updates/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Issues/ })).toBeTruthy();
  });

  it('clicking Issues activates the issues quick-filter (aria-pressed toggles)', async () => {
    render(InstalledToolbar, { props: base() });
    const issues = screen.getByRole('button', { name: /Issues/ });
    expect(issues.getAttribute('aria-pressed')).toBe('false');
    await fireEvent.click(issues);
    // quickFilter is $bindable; the component writes it locally and re-renders,
    // so aria-pressed flips even without a parent binding.
    expect(issues.getAttribute('aria-pressed')).toBe('true');
  });

  it('clicking an active chip toggles it back to all', async () => {
    render(InstalledToolbar, { props: { ...base(), quickFilter: 'updates' } });
    const updates = screen.getByRole('button', { name: /Updates/ });
    expect(updates.getAttribute('aria-pressed')).toBe('true');
    await fireEvent.click(updates);
    expect(updates.getAttribute('aria-pressed')).toBe('false');
  });

  it('hides chips when there are no updates or issues', () => {
    render(InstalledToolbar, {
      props: {
        ...base(),
        counts: { total: 3, enabled: 3, disabled: 0, updates: 0, issues: 0 },
      },
    });
    expect(screen.queryByRole('button', { name: /Updates/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /Issues/ })).toBeNull();
  });
});
