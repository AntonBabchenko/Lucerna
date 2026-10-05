import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ModpackUpdateDiff } from '$lib/ipc/bindings';
import ModpackUpdateDialog from '$lib/modpacks/ModpackUpdateDialog.svelte';

const PLAIN: ModpackUpdateDiff = {
  added: [],
  removed: [],
  updated: [],
  version_bump: null,
  new_version_number: '2.0',
};

const BUMP: ModpackUpdateDiff = {
  ...PLAIN,
  version_bump: {
    old_game_version: '1.20.1',
    new_game_version: '1.21.1',
    old_loader_version: '0.16.0',
    new_loader_version: '0.16.5',
  },
};

function box(): HTMLInputElement | null {
  return screen.queryByTestId('update-backup-worlds') as HTMLInputElement | null;
}

describe('ModpackUpdateDialog — back up worlds first', () => {
  beforeAll(() => locale.set('en'));

  it('is ticked for a Minecraft change and names how many worlds there are', () => {
    render(ModpackUpdateDialog, {
      props: { diff: BUMP, worldCount: 2, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(box()?.checked).toBe(true);
    expect(box()?.closest('label')?.textContent).toContain('(2)');
    // The hint is the checkbox's description, not part of its name.
    expect(box()?.getAttribute('aria-describedby')).toBe('update-backup-worlds-hint');
  });

  it('is offered but not ticked for an update that changes nothing risky', () => {
    render(ModpackUpdateDialog, {
      props: { diff: PLAIN, worldCount: 1, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(box()?.checked).toBe(false);
  });

  it('is hidden without worlds, and the update then backs up nothing', async () => {
    const onConfirm = vi.fn();
    render(ModpackUpdateDialog, {
      props: { diff: BUMP, worldCount: 0, onCancel: () => {}, onConfirm },
    });
    expect(box()).toBeNull();
    await fireEvent.click(screen.getByTestId('update-confirm'));
    expect(onConfirm).toHaveBeenCalledWith({ backupWorlds: false });
  });

  it('is offered without a count when the count is unknown', () => {
    render(ModpackUpdateDialog, {
      props: { diff: BUMP, worldCount: null, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(box()).not.toBeNull();
    expect(box()?.closest('label')?.textContent).not.toContain('(');
  });

  it('passes what the user chose', async () => {
    const onConfirm = vi.fn();
    render(ModpackUpdateDialog, {
      props: { diff: BUMP, worldCount: 3, onCancel: () => {}, onConfirm },
    });
    await fireEvent.click(box() as HTMLInputElement);
    await fireEvent.click(screen.getByTestId('update-confirm'));
    expect(onConfirm).toHaveBeenCalledWith({ backupWorlds: false });
  });
});
