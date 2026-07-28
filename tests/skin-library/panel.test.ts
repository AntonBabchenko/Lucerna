import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SkinLibraryPanel from '$lib/accounts/SkinLibraryPanel.svelte';
import type { SkinLibraryItem } from '$lib/ipc/bindings';

const item = (over: Partial<SkinLibraryItem> = {}): SkinLibraryItem => ({
  id: 'id-1',
  name: 'Knight',
  variant: 'classic',
  cape_id: null,
  created_at: 1,
  skin_png_base64: 'AAAA',
  ...over,
});

const two = [item(), item({ id: 'id-2', name: 'Wizard', variant: 'slim' })];

describe('SkinLibraryPanel', () => {
  it('renders a tile per entry with its name', () => {
    render(SkinLibraryPanel, { props: { entries: two, mode: 'manage' } });
    expect(screen.getByText('Knight')).toBeTruthy();
    expect(screen.getByText('Wizard')).toBeTruthy();
  });

  it('applies on tile click in manage mode', async () => {
    const onApply = vi.fn();
    render(SkinLibraryPanel, { props: { entries: two, mode: 'manage', onApply } });
    await fireEvent.click(screen.getByRole('button', { name: /wear wizard/i }));
    expect(onApply).toHaveBeenCalledExactlyOnceWith(two[1]);
  });

  it('disables tile buttons while busy', () => {
    const onApply = vi.fn();
    render(SkinLibraryPanel, { props: { entries: two, mode: 'manage', busy: true, onApply } });
    const tile = screen.getByRole('button', { name: /wear knight/i }) as HTMLButtonElement;
    expect(tile.disabled).toBe(true);
  });

  it('picks on tile click in pick mode and renders no tile menus', async () => {
    const onPick = vi.fn();
    render(SkinLibraryPanel, { props: { entries: two, mode: 'pick', onPick } });
    await fireEvent.click(screen.getByRole('button', { name: /load knight/i }));
    expect(onPick).toHaveBeenCalledExactlyOnceWith(two[0]);
    expect(screen.queryByRole('button', { name: /actions for/i })).toBeNull();
  });

  it('shows a cape badge only for entries with a saved cape', () => {
    render(SkinLibraryPanel, {
      props: {
        entries: [item({ cape_id: 'c1' })],
        mode: 'manage',
        capes: [
          { id: 'c1', alias: 'Migrator', texture_url: 'https://t.example/c1', is_active: false },
        ],
      },
    });
    expect(screen.getByText(/cape: migrator/i)).toBeTruthy();
  });

  it('deletes through the tile menu with an explicit confirm', async () => {
    const onDelete = vi.fn();
    render(SkinLibraryPanel, { props: { entries: two, mode: 'manage', onDelete } });

    await fireEvent.click(screen.getByRole('button', { name: /actions for knight/i }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /delete/i }));
    // Nothing deleted yet — the ConfirmDialog gates the destructive call.
    expect(onDelete).not.toHaveBeenCalled();
    expect(screen.getByText(/delete saved skin/i)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: /^delete$/i }));
    expect(onDelete).toHaveBeenCalledExactlyOnceWith(two[0]);
  });

  it('cancelling the delete confirm keeps the entry', async () => {
    const onDelete = vi.fn();
    render(SkinLibraryPanel, { props: { entries: two, mode: 'manage', onDelete } });

    await fireEvent.click(screen.getByRole('button', { name: /actions for knight/i }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /delete/i }));
    await fireEvent.click(screen.getByRole('button', { name: /^cancel$/i }));

    expect(onDelete).not.toHaveBeenCalled();
    expect(screen.queryByText(/delete saved skin/i)).toBeNull();
  });

  it('opens the edit dialog callback from the tile menu', async () => {
    const onEdit = vi.fn();
    render(SkinLibraryPanel, { props: { entries: two, mode: 'manage', onEdit } });

    await fireEvent.click(screen.getByRole('button', { name: /actions for wizard/i }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /edit/i }));
    expect(onEdit).toHaveBeenCalledExactlyOnceWith(two[1]);
  });

  it('shows mode-specific empty states', () => {
    const { unmount } = render(SkinLibraryPanel, { props: { entries: [], mode: 'manage' } });
    expect(screen.getByText(/save your current look/i)).toBeTruthy();
    unmount();

    render(SkinLibraryPanel, { props: { entries: [], mode: 'pick' } });
    expect(screen.getByText(/no saved skins yet\.$/i)).toBeTruthy();
  });
});
