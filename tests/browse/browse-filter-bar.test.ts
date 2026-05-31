import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// LayoutToggle reads browser-prefs; stub it so the bar renders in isolation.
vi.mock('$lib/mods/browser-prefs.svelte', () => ({
  browserPrefs: { layout: 'grid', pageSize: 20 },
}));

import BrowseFilterBar from '$lib/browse/BrowseFilterBar.svelte';

const SORTS = [
  { value: 'downloads', label: 'Downloads' },
  { value: 'relevance', label: 'Relevance' },
];

describe('BrowseFilterBar', () => {
  it('renders a search input with the given aria-label and testid', () => {
    render(BrowseFilterBar, {
      props: {
        searchAriaLabel: 'Search mods',
        searchPlaceholder: 'Search mods...',
        sort: 'downloads',
        sortOptions: SORTS,
        activeCount: 0,
        onSearchInput: () => {},
        onSortChange: () => {},
        onOpenDrawer: () => {},
      },
    });
    const input = screen.getByLabelText('Search mods');
    expect(input.getAttribute('type')).toBe('search');
  });

  it('fires onSearchInput on typing and onSortChange on sort change', async () => {
    const onSearchInput = vi.fn();
    const onSortChange = vi.fn();
    render(BrowseFilterBar, {
      props: {
        searchAriaLabel: 'Search mods',
        searchPlaceholder: 'Search mods...',
        sort: 'downloads',
        sortOptions: SORTS,
        sortTestid: 'mod-sort',
        activeCount: 0,
        onSearchInput,
        onSortChange,
        onOpenDrawer: () => {},
      },
    });
    const input = screen.getByLabelText('Search mods') as HTMLInputElement;
    input.value = 'sodium';
    await fireEvent.input(input);
    expect(onSearchInput).toHaveBeenCalledWith('sodium');

    const sort = screen.getByTestId('mod-sort') as HTMLSelectElement;
    sort.value = 'relevance';
    await fireEvent.change(sort);
    expect(onSortChange).toHaveBeenCalledWith('relevance');
  });

  it('shows the active-filter count badge only when > 0 and opens the drawer', async () => {
    const onOpenDrawer = vi.fn();
    const { rerender } = render(BrowseFilterBar, {
      props: {
        searchAriaLabel: 'Search mods',
        searchPlaceholder: 'Search mods...',
        sort: 'downloads',
        sortOptions: SORTS,
        activeCount: 0,
        onSearchInput: () => {},
        onSortChange: () => {},
        onOpenDrawer,
      },
    });
    const btn = screen.getByTestId('browse-filters-button');
    expect(btn.textContent).not.toMatch(/\d/);
    await fireEvent.click(btn);
    expect(onOpenDrawer).toHaveBeenCalled();

    await rerender({
      searchAriaLabel: 'Search mods',
      searchPlaceholder: 'Search mods...',
      sort: 'downloads',
      sortOptions: SORTS,
      activeCount: 2,
      onSearchInput: () => {},
      onSortChange: () => {},
      onOpenDrawer,
    });
    expect(screen.getByTestId('browse-filters-button').textContent).toMatch(/2/);
  });
});
