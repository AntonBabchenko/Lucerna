import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ModpackUpdateDiff } from '$lib/ipc/bindings';
import PackPlatformRestoreDialog from '$lib/modpacks/PackPlatformRestoreDialog.svelte';

// Re-applying the installed pack version: no file changes, the instance's
// Minecraft and loader go back to the pack's (Fabulously Optimized 14.0.0-beta.4
// in an instance re-versioned to 26.1.1).
const RESTORE: ModpackUpdateDiff = {
  added: [],
  removed: [],
  updated: [],
  version_bump: {
    old_game_version: '26.1.1',
    new_game_version: '26.2',
    old_loader_version: '0.19.3',
    new_loader_version: '0.19.3',
  },
  new_version_number: '14.0.0-beta.4',
};

function renderDialog(
  diff: ModpackUpdateDiff,
  onConfirm = vi.fn(),
  onCancel = vi.fn(),
  worldCount: number | null = 0,
) {
  render(PackPlatformRestoreDialog, {
    props: { diff, packName: 'Fabulously Optimized', busy: false, worldCount, onConfirm, onCancel },
  });
  return { onConfirm, onCancel };
}

describe('PackPlatformRestoreDialog', () => {
  beforeAll(() => locale.set('en'));

  it('names the Minecraft version it restores and warns about the world risk', () => {
    renderDialog(RESTORE);
    expect(screen.getByRole('heading').textContent).toContain('Restore Minecraft 26.2');
    expect(screen.getByTestId('switch-risk-mc-change').textContent).toContain('26.1.1');
    expect(screen.getByTestId('switch-risk-mc-change').textContent).toContain('26.2');
    // Same loader version on both sides — no loader warning.
    expect(screen.queryByTestId('switch-risk-loader-change')).toBeNull();
  });

  it('speaks of the loader when Minecraft stays the same', () => {
    renderDialog({
      ...RESTORE,
      version_bump: {
        old_game_version: '1.21.1',
        new_game_version: '1.21.1',
        old_loader_version: '0.16.9',
        new_loader_version: '21.1.235',
      },
    });
    expect(screen.getByRole('heading').textContent).toContain("Restore the modpack's loader");
    expect(screen.getByRole('heading').textContent).not.toContain('Minecraft');
  });

  it('never calls re-applying the installed version a downgrade', () => {
    renderDialog(RESTORE);
    expect(screen.queryByTestId('switch-risk-downgrade')).toBeNull();
  });

  it('hides the file list when no file changes', () => {
    renderDialog(RESTORE);
    expect(screen.queryByTestId('update-diff-list')).toBeNull();
  });

  it('shows the file list when the pack does change files', () => {
    renderDialog({
      ...RESTORE,
      // Only the fields ModpackDiffList renders; the full file records carry
      // download metadata these assertions never look at.
      updated: [
        {
          old: { name: 'a-1.jar', install_path: 'mods/a-1.jar', sha1: 'aa', project_id: 'a' },
          new: { name: 'a-2.jar', install_path: 'mods/a-2.jar', sha1: 'bb', project_id: 'a' },
        },
      ] as unknown as ModpackUpdateDiff['updated'],
    });
    expect(screen.getByTestId('update-diff-list')).toBeTruthy();
    // The sentence above the list must not promise the opposite of the list.
    const dialog = screen.getByTestId('pack-restore-dialog').textContent ?? '';
    expect(dialog).not.toContain('no mod files change');
    expect(dialog).toContain('these files change');
  });

  it('promises unchanged mod files only when the diff has none', () => {
    renderDialog(RESTORE);
    expect(screen.getByTestId('pack-restore-dialog').textContent).toContain('no mod files change');
  });

  it('says there is nothing to restore when the archive already matches the instance', () => {
    renderDialog({ ...RESTORE, version_bump: null });
    expect(screen.getByTestId('pack-restore-already-matches').textContent).toContain(
      'already runs',
    );
    expect(screen.queryByTestId('pack-restore-confirm')).toBeNull();
  });

  it('confirms and cancels through its callbacks', async () => {
    const { onConfirm, onCancel } = renderDialog(RESTORE);
    await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  // «Back up worlds first» — the choice the update and switch confirms offer,
  // for the same reason: the worlds move to another Minecraft version.
  describe('the world backup', () => {
    it('is offered and on by default when the profile has worlds', async () => {
      const { onConfirm } = renderDialog(RESTORE, vi.fn(), vi.fn(), 3);
      const box = screen.getByTestId('restore-backup-worlds') as HTMLInputElement;
      expect(box.checked).toBe(true);
      expect(screen.getByTestId('pack-restore-dialog').textContent).toContain('3');
      await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
      expect(onConfirm).toHaveBeenCalledWith({ backupWorlds: true });
    });

    it('confirms without a backup once the user unticks it', async () => {
      const { onConfirm } = renderDialog(RESTORE, vi.fn(), vi.fn(), 3);
      await fireEvent.click(screen.getByTestId('restore-backup-worlds'));
      await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
      expect(onConfirm).toHaveBeenCalledWith({ backupWorlds: false });
    });

    it('is still offered when the world count could not be read', () => {
      renderDialog(RESTORE, vi.fn(), vi.fn(), null);
      expect(screen.getByTestId('restore-backup-worlds')).toBeTruthy();
    });

    it('is not offered, and not asked for, when the profile has no worlds', async () => {
      const { onConfirm } = renderDialog(RESTORE, vi.fn(), vi.fn(), 0);
      expect(screen.queryByTestId('restore-backup-worlds')).toBeNull();
      await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
      expect(onConfirm).toHaveBeenCalledWith({ backupWorlds: false });
    });

    it('is not offered when there is nothing to restore', () => {
      renderDialog({ ...RESTORE, version_bump: null }, vi.fn(), vi.fn(), 3);
      expect(screen.queryByTestId('restore-backup-worlds')).toBeNull();
    });
  });
});
