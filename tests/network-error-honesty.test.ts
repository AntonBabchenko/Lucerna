// v0.28 design review, E2–E4: a failed list says what failed and can be tried again; the
// version picker says «loading» or «failed» rather than blaming the connection for everything.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
import { commands } from '$lib/ipc/bindings';
import { createMcVersions } from '$lib/versions/mc-versions.svelte';
import VersionListNotice from '$lib/versions/VersionListNotice.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listVersions: vi.fn(),
    listFabricLoaders: vi.fn(),
    listQuiltLoaders: vi.fn(),
    listForgeLoaders: vi.fn(),
    listNeoforgeLoaders: vi.fn(),
  },
}));

const netFailure = {
  kind: 'network',
  url: 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json',
  failure: { kind: 'http_status', status: 503 },
  details: '503',
} as never;

beforeAll(() => locale.set('en'));

describe('the version list under a picker that shows no version', () => {
  it('says it is loading while it loads, not that the connection failed', () => {
    render(VersionListNotice, {
      total: 0,
      loading: true,
      failure: null,
      onReload: vi.fn(),
      pickerId: 'picker',
    });
    expect(screen.getByTestId('version-list-loading')).toBeTruthy();
    expect(screen.queryByText(/check your connection/)).toBeNull();
  });

  it('a failed load says why and offers Reload', async () => {
    const onReload = vi.fn();
    render(VersionListNotice, {
      total: 0,
      loading: false,
      failure:
        "Couldn't load the Minecraft version list. piston-meta.mojang.com is having problems.",
      onReload,
      pickerId: 'picker',
    });
    expect(screen.getByText(/Couldn't load the Minecraft version list/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    expect(onReload).toHaveBeenCalledOnce();
  });

  it('a loaded empty list says so, without blaming the connection', () => {
    render(VersionListNotice, {
      total: 0,
      loading: false,
      failure: null,
      onReload: vi.fn(),
      pickerId: 'picker',
    });
    expect(screen.getByText('No Minecraft versions are available.')).toBeTruthy();
  });
});

describe('the page’s version list', () => {
  it('a failure names what failed, and survives closing the Overview banner', async () => {
    vi.mocked(commands.listVersions).mockResolvedValue({ status: 'error', error: netFailure });
    const mcv = createMcVersions();
    await mcv.load();
    expect(mcv.error).toMatch(/^Couldn't load the Minecraft version list\. /);
    mcv.dismissError();
    expect(mcv.error).toBeNull();
    expect(mcv.failure).toMatch(/^Couldn't load the Minecraft version list\. /);
    vi.mocked(commands.listVersions).mockResolvedValue({ status: 'ok', data: [] });
    await mcv.load();
    expect(mcv.failure).toBeNull();
    mcv.dispose();
  });
});

describe('the loader picker’s failed list', () => {
  it('a network failure offers Reload, which fetches again and hands the focus to the list', async () => {
    vi.mocked(commands.listForgeLoaders)
      .mockResolvedValueOnce({ status: 'error', error: netFailure })
      .mockResolvedValueOnce({
        status: 'ok',
        data: [{ version: '47.3.0', stable: true }],
      } as never);
    render(LoaderPicker, { mc: '1.20.1', loader: 'forge', loaderVersion: null });
    const reload = await screen.findByRole('button', { name: 'Reload' });
    reload.focus();
    await fireEvent.click(reload);
    await waitFor(() => expect(commands.listForgeLoaders).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Reload' })).toBeNull());
    // The keyboard user lands on the version list the reload filled, not on <body>.
    await waitFor(() => expect(document.activeElement?.getAttribute('role')).toBe('combobox'));
  });

  it('a definitive answer offers no Reload', async () => {
    vi.mocked(commands.listQuiltLoaders).mockResolvedValue({
      status: 'error',
      error: { kind: 'loader_unavailable', loader: 'quilt', mc_version: '26.1.2' },
    } as never);
    render(LoaderPicker, { mc: '26.1.2', loader: 'quilt', loaderVersion: null });
    expect(await screen.findByText(/Quilt/)).toBeTruthy();
    await waitFor(() => expect(commands.listQuiltLoaders).toHaveBeenCalled());
    expect(screen.queryByRole('button', { name: 'Reload' })).toBeNull();
  });
});
