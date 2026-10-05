import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

// Real-browser regression test for the review of #475: the Minecraft version field's options were
// plain buttons in the Tab order, so Tab walked into the list instead of on to the next control, and
// the list stayed open once the focus had left. The unit tests drive happy-dom with user-event's
// own tab order and focus rules; here Chromium moves the focus and fires the events.

const RELEASES = ['1.21.1', '1.21', '1.20.4', '1.20.1'].map((id) => ({
  id,
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: `https://piston-meta.mojang.com/v1/packages/${id}.json`,
}));

test.beforeEach(async ({ page }) => {
  await installMockIpc(page, {
    instances: [
      makeInstance({ id: 'inst-1', name: 'Fabric', loader: 'fabric', loader_version: '0.15.0' }),
    ],
    active_instance_id: 'inst-1',
    versions: RELEASES,
  });
  await page.goto('/');
  // The Add-ons tab opens on the mod browser: Search · Loader · MC · Sort · …
  await page.getByRole('tab', { name: 'Add-ons' }).click();
});

test('Tab moves on from the MC version field and closes its list', async ({ page }) => {
  const field = page.getByRole('combobox', { name: 'MC' });
  const list = page.getByRole('listbox');
  await field.click();
  await expect(list).toBeVisible();

  await page.keyboard.press('Tab');
  await expect(page.getByRole('combobox', { name: /Sort/ })).toBeFocused();
  await expect(list).toBeHidden();

  // Back into the field: its focus opens the list again. One more: the loader, the list closed.
  await page.keyboard.press('Shift+Tab');
  await expect(field).toBeFocused();
  await expect(list).toBeVisible();
  await page.keyboard.press('Shift+Tab');
  await expect(page.getByRole('combobox', { name: /Loader/ })).toBeFocused();
  await expect(list).toBeHidden();
});

test('a press on a version keeps the focus in the field, and the click picks it', async ({
  page,
}) => {
  const field = page.getByRole('combobox', { name: 'MC' });
  const list = page.getByRole('listbox');
  // The field opens with the profile's version in it; empty, it lists every release.
  await field.fill('');
  await expect(list).toBeVisible();

  await page.getByRole('option', { name: '1.21', exact: true }).hover();
  await page.mouse.down();
  await expect(field).toBeFocused();
  await expect(list).toBeVisible();
  await page.mouse.up();

  await expect(field).toHaveValue('1.21');
  await expect(list).toBeHidden();
});
