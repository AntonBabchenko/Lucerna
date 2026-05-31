import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import BrowseFilterChips from '$lib/browse/BrowseFilterChips.svelte';

describe('BrowseFilterChips', () => {
  it('renders nothing when there are no chips', () => {
    const { container } = render(BrowseFilterChips, {
      props: { chips: [], onClear: () => {}, onClearAll: () => {}, clearAllTestid: 'x-clear' },
    });
    expect(container.querySelector('[data-testid="browse-filter-chips"]')).toBeNull();
  });

  it('renders one chip per entry plus a Clear all button with the given testid', () => {
    render(BrowseFilterChips, {
      props: {
        chips: [
          { key: 'loader', label: 'Fabric' },
          { key: 'mc', label: '1.21.1' },
        ],
        onClear: () => {},
        onClearAll: () => {},
        clearAllTestid: 'mod-clear-filters',
      },
    });
    expect(screen.getByTestId('browse-chip-loader').textContent).toMatch(/Fabric/);
    expect(screen.getByTestId('browse-chip-mc').textContent).toMatch(/1\.21\.1/);
    const clearAll = screen.getByTestId('mod-clear-filters');
    expect(clearAll).toHaveBtnVariant('tertiary');
    expect(clearAll.className).toContain('text-xs');
  });

  it('clicking a chip calls onClear with its key', async () => {
    const onClear = vi.fn();
    render(BrowseFilterChips, {
      props: {
        chips: [{ key: 'loader', label: 'Fabric' }],
        onClear,
        onClearAll: () => {},
        clearAllTestid: 'x-clear',
      },
    });
    await fireEvent.click(screen.getByTestId('browse-chip-loader'));
    expect(onClear).toHaveBeenCalledWith('loader');
  });

  it('clicking Clear all calls onClearAll', async () => {
    const onClearAll = vi.fn();
    render(BrowseFilterChips, {
      props: {
        chips: [{ key: 'loader', label: 'Fabric' }],
        onClear: () => {},
        onClearAll,
        clearAllTestid: 'x-clear',
      },
    });
    await fireEvent.click(screen.getByTestId('x-clear'));
    expect(onClearAll).toHaveBeenCalled();
  });
});
