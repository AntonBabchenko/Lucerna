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

const base = {
  searchAriaLabel: 'Search mods',
  searchPlaceholder: 'Search mods...',
  sort: 'downloads',
  sortOptions: SORTS,
  onSearchInput: () => {},
  onSortChange: () => {},
};

describe('BrowseFilterBar (inline facets)', () => {
  it('renders a search input with the given aria-label', () => {
    render(BrowseFilterBar, { props: { ...base } });
    const input = screen.getByLabelText('Search mods');
    expect(input.getAttribute('type')).toBe('search');
  });

  it('fires onSearchInput on typing and onSortChange on sort change', async () => {
    const onSearchInput = vi.fn();
    const onSortChange = vi.fn();
    render(BrowseFilterBar, {
      props: { ...base, sortTestid: 'mod-sort', onSearchInput, onSortChange },
    });
    const input = screen.getByLabelText('Search mods') as HTMLInputElement;
    input.value = 'sodium';
    await fireEvent.input(input);
    expect(onSearchInput).toHaveBeenCalledWith('sodium');

    const sort = screen.getByTestId('mod-sort');
    await fireEvent.click(sort);
    await fireEvent.mouseDown(screen.getByRole('option', { name: /relevance/i }));
    expect(onSortChange).toHaveBeenCalledWith('relevance');
  });

  it('shows the loader dropdown inline only when showLoader is set', () => {
    const { rerender } = render(BrowseFilterBar, { props: { ...base } });
    expect(screen.queryByTestId('browse-loader-select')).toBeNull();
    rerender({ ...base, showLoader: true });
    expect(screen.getByTestId('browse-loader-select')).toBeTruthy();
  });

  it('renders the source dropdown inline when source + options are supplied', () => {
    render(BrowseFilterBar, {
      props: {
        ...base,
        source: 'modrinth',
        sourceOptions: [
          { value: 'modrinth', label: 'Modrinth' },
          { value: 'curseforge', label: 'CurseForge' },
        ],
      },
    });
    expect(screen.getByTestId('browse-source-select')).toBeTruthy();
  });

  it('fires onShowInstalledChange when the checkbox is toggled', async () => {
    const onShowInstalledChange = vi.fn();
    render(BrowseFilterBar, {
      props: { ...base, showInstalled: true, onShowInstalledChange },
    });
    const checkbox = screen.getByTestId('browse-show-installed') as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
    await fireEvent.click(checkbox);
    expect(onShowInstalledChange).toHaveBeenCalledWith(false);
  });

  it('shows a Clear-all button only when activeCount > 0 and fires onClearAll', async () => {
    const onClearAll = vi.fn();
    const { rerender } = render(BrowseFilterBar, {
      props: { ...base, activeCount: 0, onClearAll },
    });
    expect(screen.queryByTestId('browse-clear-filters')).toBeNull();
    await rerender({ ...base, activeCount: 2, onClearAll });
    const clear = screen.getByTestId('browse-clear-filters');
    await fireEvent.click(clear);
    expect(onClearAll).toHaveBeenCalled();
  });

  it('shows the Restore button only when canRestore and fires onRestore', async () => {
    const onRestore = vi.fn();
    render(BrowseFilterBar, {
      props: { ...base, canRestore: true, restoreLabel: 'Match this instance', onRestore },
    });
    const restore = screen.getByTestId('browse-restore-instance');
    await fireEvent.click(restore);
    expect(onRestore).toHaveBeenCalled();
  });
});
