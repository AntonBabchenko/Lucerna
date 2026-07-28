import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

// Regression spec for the doubled-LucernaData incident: picking a folder that
// already contains a Lucerna data root must offer ADOPT (repoint, no copy) and
// commit through adopt_data_location — never through set_data_location, which
// would migrate a fresh empty root into a nested LucernaData\LucernaData and
// abandon the real data.
const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

test('picking an existing Lucerna root offers adopt and calls adopt_data_location', async ({
  page,
}) => {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [makeInstance({ id: 'inst-1', name: 'Default' })],
    active_instance_id: 'inst-1',
    data_location: { effective: 'C:\\Empty\\Default', configured: null, fell_back: false },
    picked_directory: 'C:\\Programs\\Lucerna',
    data_location_plan: { kind: 'adopt', path: 'C:\\Programs\\Lucerna\\LucernaData' },
  });
  await page.goto('/');

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('tab', { name: 'Storage' }).click();
  await page.getByRole('button', { name: 'Change location…' }).click();

  // Adopt dialog — not the move dialog — naming the found root and warning
  // that the current data stays on disk.
  await expect(page.getByText('Use existing data folder?')).toBeVisible();
  await expect(page.getByText(/already contains Lucerna data/)).toBeVisible();
  await expect(page.getByText(/will stay on disk/)).toBeVisible();

  await page.getByRole('button', { name: 'Switch and restart' }).click();
  await expect(page.getByText('Use existing data folder?')).toBeHidden();

  const calls = await page.evaluate(
    () =>
      (window as unknown as { __mockIpcCalls?: Array<{ cmd: string; args: unknown }> })
        .__mockIpcCalls ?? [],
  );
  const adopt = calls.filter((c) => c.cmd === 'adopt_data_location');
  expect(adopt).toHaveLength(1);
  expect(adopt[0]?.args).toMatchObject({ path: 'C:\\Programs\\Lucerna\\LucernaData' });
  expect(calls.some((c) => c.cmd === 'set_data_location')).toBe(false);
});

test('picking a plain folder keeps the classic move flow', async ({ page }) => {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [makeInstance({ id: 'inst-1', name: 'Default' })],
    active_instance_id: 'inst-1',
    picked_directory: 'D:\\Games',
    data_location_plan: { kind: 'migrate', path: 'D:\\Games\\LucernaData' },
  });
  await page.goto('/');

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('tab', { name: 'Storage' }).click();
  await page.getByRole('button', { name: 'Change location…' }).click();

  await expect(page.getByText('Move data folder?')).toBeVisible();
  await page.getByRole('button', { name: 'Move and restart' }).click();
  await expect(page.getByText('Move data folder?')).toBeHidden();

  const calls = await page.evaluate(
    () =>
      (window as unknown as { __mockIpcCalls?: Array<{ cmd: string; args: unknown }> })
        .__mockIpcCalls ?? [],
  );
  const migrate = calls.filter((c) => c.cmd === 'set_data_location');
  expect(migrate).toHaveLength(1);
  expect(migrate[0]?.args).toMatchObject({ newPath: 'D:\\Games\\LucernaData' });
  expect(calls.some((c) => c.cmd === 'adopt_data_location')).toBe(false);
});
