import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// McVersionCombobox reads the mcVersions rune; stub it to an empty list.
vi.mock('$lib/settings/state.svelte', () => ({ mcVersions: { value: [] } }));

import BrowseFilterDrawer from '$lib/browse/BrowseFilterDrawer.svelte';

describe('BrowseFilterDrawer', () => {
  it('renders nothing when closed', () => {
    render(BrowseFilterDrawer, { props: { open: false, loader: '', mc: '' } });
    expect(screen.queryByTestId('browse-filter-drawer')).toBeNull();
  });

  it('open drawer is a labelled modal dialog with a loader radiogroup', () => {
    render(BrowseFilterDrawer, { props: { open: true, loader: '', mc: '' } });
    const dialog = screen.getByRole('dialog', { name: /filters/i });
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(screen.getByRole('radiogroup', { name: /loader filter/i })).toBeTruthy();
  });

  it('omits the source field by default and the show-installed field when no handler', () => {
    render(BrowseFilterDrawer, { props: { open: true, loader: '', mc: '' } });
    expect(screen.queryByRole('radiogroup', { name: /mod source/i })).toBeNull();
    expect(screen.queryByLabelText(/show installed/i)).toBeNull();
  });

  it('renders the source segmented control when a source value is provided', () => {
    render(BrowseFilterDrawer, {
      props: { open: true, loader: '', mc: '', source: 'modrinth' },
    });
    expect(screen.getByRole('radiogroup', { name: /mod source/i })).toBeTruthy();
  });

  it('renders the show-installed toggle and fires its handler', async () => {
    const onShowInstalledChange = vi.fn();
    render(BrowseFilterDrawer, {
      props: { open: true, loader: '', mc: '', showInstalled: true, onShowInstalledChange },
    });
    const toggle = screen.getByLabelText(/show installed/i);
    await fireEvent.click(toggle);
    expect(onShowInstalledChange).toHaveBeenCalledWith(false);
  });

  it('Escape closes the drawer', async () => {
    const { component } = render(BrowseFilterDrawer, { props: { open: true, loader: '', mc: '' } });
    await fireEvent.keyDown(document, { key: 'Escape' });
    // `open` is bindable; the dialog should be gone after the state flips.
    expect(screen.queryByTestId('browse-filter-drawer')).toBeNull();
    void component;
  });
});
