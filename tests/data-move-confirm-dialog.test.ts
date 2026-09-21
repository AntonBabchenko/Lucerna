import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import DataLocationConfirmDialog from '$lib/settings/DataLocationConfirmDialog.svelte';

const MIB = 1024 ** 2;
const GIB = 1024 ** 3;
const base = {
  mode: 'move' as const,
  fromPath: 'C:\\Old',
  toPath: 'D:\\Games\\LucernaData',
  requiredBytes: 4096,
  freeBytes: 10 * GIB,
  busy: false,
  onCancel: vi.fn(),
  onConfirm: vi.fn(),
};
const mount = (over: Record<string, unknown> = {}) =>
  render(DataLocationConfirmDialog, { props: { ...base, ...over } });
const text = (testid: string) => screen.getByTestId(testid).textContent?.trim();
const has = (testid: string) => screen.queryByTestId(testid) !== null;
const confirmBtn = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

describe('DataLocationConfirmDialog — move', () => {
  it('names both folders in full, the size, the free space, and what happens next', () => {
    mount();
    expect(screen.getByText('Move data folder?')).toBeTruthy();
    expect(text('data-move-from')).toBe('C:\\Old');
    expect(text('data-move-to')).toBe('D:\\Games\\LucernaData');
    expect(text('data-move-size')).toBe('about 4.0 KB');
    expect(text('data-move-free')).toBe('10.00 GB');
    expect(screen.getByText(/You can cancel while files are being copied/)).toBeTruthy();
    expect(screen.getByText(/LucernaData subfolder of the folder you picked/)).toBeTruthy();
    expect(has('data-move-low-space') || has('data-move-free-unknown')).toBe(false);
  });

  it('warns with both numbers when space looks short — and still lets the user go ahead', () => {
    mount({ requiredBytes: 10 * GIB, freeBytes: 10 * GIB + MIB });
    expect(text('data-move-low-space')).toContain('about 10.00 GB to copy, 10.00 GB free');
    expect(text('data-move-low-space')).toContain('pick it here to switch without copying');
    expect(confirmBtn('Move and restart').disabled).toBe(false);
  });

  it.each([
    [GIB + 64 * MIB, false], // exactly required + max(required / 50, 64 MiB)
    [GIB + 64 * MIB - 1, true],
  ])('draws the low-space line at the 64 MiB floor (free=%i → %s)', (freeBytes, low) => {
    mount({ requiredBytes: GIB, freeBytes });
    expect(has('data-move-low-space')).toBe(low);
  });

  it('says so when free space could not be checked, without blocking', () => {
    mount({ freeBytes: null });
    expect(text('data-move-free-unknown')).toContain("couldn't check how much free space");
    expect(has('data-move-free') || has('data-move-low-space')).toBe(false);
    expect(confirmBtn('Move and restart').disabled).toBe(false);
  });

  it('never prints "0 B" for a size it does not know', () => {
    mount({ requiredBytes: null });
    expect(has('data-move-size')).toBe(false);
    expect(screen.queryByText(/\b0 B\b/)).toBeNull();
  });
});

describe('DataLocationConfirmDialog — reset and adopt', () => {
  it('names the default folder a reset moves the data back to', () => {
    mount({ mode: 'reset', toPath: 'C:\\Users\\u\\AppData\\Roaming\\com.lucerna.app' });
    expect(screen.getByText('Reset to the default location?')).toBeTruthy();
    expect(text('data-move-to')).toBe('C:\\Users\\u\\AppData\\Roaming\\com.lucerna.app');
    expect(screen.queryByText(/LucernaData subfolder/)).toBeNull();
    expect(confirmBtn('Move and restart')).toBeTruthy();
  });

  it('promises no move for a pointer-only reset, and no "once the move finishes"', () => {
    mount({ mode: 'reset', pointerOnly: true, detachedPath: 'D:\\Dead', requiredBytes: 0 });
    expect(screen.getByText(/detaches the unavailable folder "D:\\Dead"/)).toBeTruthy();
    expect(screen.getByText('Nothing is copied. The app restarts right away.')).toBeTruthy();
    expect(has('data-move-from')).toBe(false);
    expect(screen.queryByText(/finishes the switch|move finishes/)).toBeNull();
    expect(confirmBtn('Detach and restart')).toBeTruthy();
  });

  it('keeps the pinned adopt phrases and gives the current size only when it is known', () => {
    const { unmount } = mount({
      mode: 'adopt',
      toPath: 'C:\\P\\LucernaData',
      currentSizeBytes: 4096,
    });
    expect(screen.getByText('Use existing data folder?')).toBeTruthy();
    expect(screen.getByText(/already contains Lucerna data/)).toBeTruthy();
    expect(screen.getByText(/4\.0 KB at "C:\\Old"\) will stay on disk/)).toBeTruthy();
    expect(confirmBtn('Switch and restart')).toBeTruthy();
    unmount();
    mount({ mode: 'adopt', toPath: 'C:\\P\\LucernaData', currentSizeBytes: null });
    expect(screen.getByText(/Your current data \(at "C:\\Old"\) will stay on disk/)).toBeTruthy();
  });
});
