import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};
const baseInstance = makeInstance({ id: 'inst-1', name: 'Default' });

async function openSettings(page: import('@playwright/test').Page) {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [baseInstance],
    active_instance_id: 'inst-1',
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  return dialog;
}

test('search jumps to a setting and flashes it', async ({ page }) => {
  const dialog = await openSettings(page);

  // Anchor choice: `game.gpu`'s control only renders when GamePanel's
  // gpuCapability command resolves to `{ kind: 'available', … }`. mock-ipc.ts's
  // handler table has no `gpu_capability` entry, so it falls through to
  // `__default: () => null` — the GPU block never mounts under the mock. Use
  // `appearance.language` instead: it is always present (AppearancePanel has no
  // conditional gate around it) and lands on the section the modal already
  // opens on, so this also verifies the same-section jump path.
  await dialog.getByTestId('settings-search-input').fill('language');
  // First (only) result is the "Interface language" row under Appearance.
  const result = dialog.locator('[data-search-result="appearance.language"]');
  await expect(result).toBeVisible();

  await dialog.getByTestId('settings-search-input').press('Enter');

  // The language control's wrapper scrolls into view and carries the flash class.
  const target = dialog.locator('[data-search-anchor="appearance.language"]');
  await expect(target).toBeVisible();
  await expect(target).toHaveClass(/field-flash/);
});

test('empty results show the no-results row; Escape clears the query', async ({ page }) => {
  const dialog = await openSettings(page);
  const input = dialog.getByTestId('settings-search-input');

  await input.fill('zzzzzzzz');
  await expect(dialog.getByTestId('settings-search-empty')).toBeVisible();

  await input.press('Escape');
  await expect(input).toHaveValue('');
  // Back to the section tablist.
  await expect(dialog.getByRole('tab', { name: 'Appearance' })).toBeVisible();
});
