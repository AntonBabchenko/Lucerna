import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    restoreBackup: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { final_folder_name: 'W' },
    }),
  },
}));

const backup = {
  filename: '2026-05-24T15-30-12.zip',
  size_bytes: 1024,
  created_unix_ms: Date.now(),
};

describe('RestoreBackupDialog', () => {
  it('defaults to replace mode', () => {
    const { getByRole } = render(RestoreBackupDialog, {
      props: {
        instanceId: 'i1',
        worldFolder: 'W',
        backup,
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const replaceRadio = getByRole('radio', { name: /Replace current world/ });
    expect((replaceRadio as HTMLInputElement).checked).toBe(true);
  });

  it('fires restoreBackup with as_copy when that radio is selected', async () => {
    const mod = await import('$lib/ipc/bindings');
    const { getByRole, getByText } = render(RestoreBackupDialog, {
      props: {
        instanceId: 'i1',
        worldFolder: 'W',
        backup,
        onClose: () => {},
        onRestored: () => {},
      },
    });
    await fireEvent.click(getByRole('radio', { name: /Restore as a copy/ }));
    await fireEvent.click(getByText('Restore'));
    expect(mod.commands.restoreBackup).toHaveBeenCalledWith('i1', 'W', backup.filename, 'as_copy');
  });
});
