import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

// Real-keyboard regression test for the 2026-10-02 regression finding F05: Escape on the open
// Minecraft-version list ended the Add-ons tour running underneath it (and, in the launcher-import
// dialog, closed the whole dialog). The list closed without consuming the key and relied on its
// layer still being the top one when the window's layer router ran. For a REAL key press the
// browser runs microtasks between the two listeners — Svelte's flush among them, which releases the
// list's layer — so the router handed the same Escape to the tour. A synthetic `dispatchEvent` (the
// unit tests) has no such checkpoint, which is why only a real browser shows it.

test('Escape on the open MC version list closes the list, not the tour under it', async ({
  page,
}) => {
  await installMockIpc(page, {
    instances: [
      makeInstance({ id: 'inst-1', name: 'Fabric', loader: 'fabric', loader_version: '0.15.0' }),
    ],
    active_instance_id: 'inst-1',
    pending_tours: ['addons'],
  });
  await page.goto('/');

  // The Add-ons tab opens on the mod browser, and its tour fires on the first visit.
  await page.getByRole('tab', { name: 'Add-ons' }).click();
  const tour = page.getByTestId('contextual-tour-popover');
  await expect(tour).toBeVisible();

  // Focusing the field opens its list on top of the tour, which steps aside meanwhile. (Focus,
  // not a click: the tour's card may sit over the field.)
  await page.getByRole('combobox', { name: 'MC' }).focus();
  await expect(page.getByRole('listbox')).toBeVisible();
  await expect(tour).toBeHidden();

  // One Escape closes one thing: the list. The tour comes back on the same step.
  await page.keyboard.press('Escape');
  await expect(page.getByRole('listbox')).toBeHidden();
  await expect(tour).toBeVisible();
  await expect(tour).toContainText('Step 1 of 4');

  // The next one reaches the tour.
  await page.keyboard.press('Escape');
  await expect(tour).toBeHidden();
});
