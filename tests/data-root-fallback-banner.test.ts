import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ restartLauncher: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: { restartLauncher: h.restartLauncher } }));

import DataRootFallbackBanner from '$lib/settings/DataRootFallbackBanner.svelte';
import { settingsOpen, settingsSearchFocus } from '$lib/settings/state.svelte';

const flush = () => new Promise((r) => setTimeout(r, 0));
const button = (name: string) => screen.queryByRole('button', { name }) as HTMLButtonElement | null;
const text = () => screen.getByTestId('data-root-fallback-banner').textContent ?? '';

beforeEach(() => {
  vi.clearAllMocks();
  settingsOpen.value = null;
  settingsSearchFocus.value = null;
  h.restartLauncher.mockResolvedValue({ status: 'ok', data: null });
});

describe('DataRootFallbackBanner', () => {
  it('says why, names the folder, and offers the two ways forward', () => {
    render(DataRootFallbackBanner, {
      props: { reason: { kind: 'root_missing' }, configuredPath: 'D:\\LucernaData' },
    });
    const banner = screen.getByTestId('data-root-fallback-banner');
    expect(banner.getAttribute('role')).toBe('alert');
    expect(text()).toContain('"D:\\LucernaData" can\'t be found');
    expect(text()).toContain('temporary session');
    expect(text()).not.toMatch(/empty/i);
    // The dominant CTA is the solid warning button (DESIGN §10); Restart is its soft twin.
    expect(button('Storage settings')?.className).toContain('btn-warning');
    expect(button('Storage settings')?.className).not.toContain('btn-warning-soft');
    expect(button('Restart')?.className).toContain('btn-warning-soft');
  });

  it('a folder that is present but read-only is not told to reconnect', () => {
    render(DataRootFallbackBanner, {
      props: {
        reason: { kind: 'root_not_writable', details: 'Access is denied.' },
        configuredPath: 'D:\\LucernaData',
      },
    });
    expect(text()).toContain("can't be written to");
    expect(text()).not.toMatch(/reconnect/i);
    // The OS error belongs to the Storage notice, not to a banner read at a glance.
    expect(text()).not.toContain('Access is denied');
  });

  it('has a sentence without a path when the pointer could not be read', () => {
    render(DataRootFallbackBanner, {
      props: { reason: { kind: 'pointer_corrupt' }, configuredPath: null },
    });
    expect(text()).toContain("couldn't read where your data folder is");
    expect(text()).not.toContain('""');
  });

  it('"Storage settings" opens Settings at Storage and points at the data-location field', async () => {
    render(DataRootFallbackBanner, {
      props: { reason: { kind: 'root_missing' }, configuredPath: 'D:\\LucernaData' },
    });
    await fireEvent.click(button('Storage settings') as HTMLButtonElement);
    await flush();
    expect(settingsOpen.value).toEqual({ tab: 'storage' });
    expect(settingsSearchFocus.value).toBe('storage.dataLocation');
  });

  it('shows a refused restart inside the banner, with the way out, and lets the user try again', async () => {
    h.restartLauncher.mockResolvedValue({
      status: 'error',
      error: { kind: 'data_location_busy' },
    });
    render(DataRootFallbackBanner, {
      props: { reason: { kind: 'root_missing' }, configuredPath: 'D:\\LucernaData' },
    });
    await fireEvent.click(button('Restart') as HTMLButtonElement);
    await flush();
    expect(h.restartLauncher).toHaveBeenCalledTimes(1);
    const error = screen.getByTestId('data-root-fallback-restart-error');
    expect(error.textContent).toContain('Close Lucerna and open it again');
    // No nested live region: the banner root already is one, and a nested `role="alert"` would
    // re-announce the whole banner.
    expect(error.getAttribute('role')).toBeNull();
    expect(button('Restart')?.disabled).toBe(false);
  });
});
