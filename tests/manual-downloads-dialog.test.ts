import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const openUrl = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: (u: string) => openUrl(u) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));

const entry = vi.hoisted(() => (over: Record<string, unknown> = {}) => ({
  reason: 'distribution_disabled',
  mod_name: 'Scape and Run',
  manual_action_url: 'https://www.curseforge.com/projects/1',
  filename: 'srp.jar',
  size: 1048576,
  sha1: 'aa',
  project_id: '1',
  meta: {
    file_id: '5',
    md5: null,
    install_path: 'mods/srp.jar',
    required: true,
    download_url: 'https://www.curseforge.com/minecraft/mc-mods/srp/download/5',
  },
  ...over,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackStatus: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        origin: { project_name: 'Big Pack', files: [] },
        installed_shas: [],
        removed_files: [],
        added_count: 0,
        is_modified: false,
        missing_mods: [
          { entry: entry(), state: 'missing' },
          {
            entry: entry({
              mod_name: 'Done Mod',
              meta: {
                file_id: '6',
                md5: null,
                install_path: null,
                required: true,
                download_url: null,
              },
            }),
            state: 'installed',
          },
        ],
        supplied_shas: [],
      },
    }),
    manualDownloadsDefaultFolder: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: 'C:/Users/u/Downloads' }),
    manualDownloadsSupply: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        rows: [{ kind: 'not_in_pack', path: 'C:/dl/x.jar', found: 'Other Mod 2.0', asked: true }],
      },
    }),
    manualDownloadsScan: vi.fn(),
    clipboardWriteText: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: { modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));

import ManualDownloadsDialog from '$lib/modpacks/manual/ManualDownloadsDialog.svelte';
import { droppedManualDownloads } from '$lib/settings/state.svelte';

afterEach(() => vi.clearAllMocks());

describe('ManualDownloadsDialog', () => {
  it('lists pending files first, with their download link', async () => {
    render(ManualDownloadsDialog, { instanceId: 'i1', onClose: () => {} });
    const rows = await screen.findAllByTestId('manual-row');
    expect(rows[0].textContent).toContain('Scape and Run');
    await fireEvent.click(screen.getAllByRole('button', { name: /^download/i })[0]);
    await waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        'https://www.curseforge.com/minecraft/mc-mods/srp/download/5',
      ),
    );
  });

  it('checks a drop and says what the refused file is', async () => {
    render(ManualDownloadsDialog, { instanceId: 'i1', onClose: () => {} });
    await screen.findAllByTestId('manual-row');
    droppedManualDownloads.value = ['C:/dl/x.jar'];
    await waitFor(() =>
      expect(screen.getByTestId('manual-result').textContent).toContain('Other Mod 2.0'),
    );
    expect(droppedManualDownloads.value).toBeNull();
  });

  it('says so when a look in the folder throws, instead of failing silently', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.manualDownloadsScan).mockRejectedValueOnce(new Error('disk gone'));
    render(ManualDownloadsDialog, { instanceId: 'i1', onClose: () => {} });
    await screen.findAllByTestId('manual-row');
    await waitFor(() =>
      expect(
        (
          screen.getByRole('button', { name: 'Look in the folder' }) as HTMLButtonElement
        ).getAttribute('aria-disabled'),
      ).not.toBe('true'),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Look in the folder' }));
    await waitFor(() =>
      expect(screen.getByTestId('manual-result').textContent).toContain('disk gone'),
    );
  });

  it('keeps the folder watch off until the user turns it on', async () => {
    render(ManualDownloadsDialog, { instanceId: 'i1', onClose: () => {} });
    const box = (await screen.findByTestId('manual-watch')) as HTMLInputElement;
    expect(box.checked).toBe(false);
  });
});
