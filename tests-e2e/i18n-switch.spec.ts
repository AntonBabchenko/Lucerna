import { expect, test } from '@playwright/test';
import { installMockIpc } from './helpers/mock-ipc';

// Mirror the shapes from settings.spec.ts (and mock-ipc.ts).
const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

const baseInstance = {
  id: 'inst-1',
  name: 'Default',
  mc_version: '1.20.4',
  loader: 'vanilla' as const,
  loader_version: null,
  max_heap_mb: 2048,
  extra_jvm_args: '',
  created_unix_ms: null,
  ready: true,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
};

const SETTINGS_DIALOG = '[role="dialog"][aria-label="Settings"]';

test('language picker switches labels live without a page reload', async ({ page }) => {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [baseInstance],
    active_instance_id: 'inst-1',
  });
  await page.goto('/');

  // Open Settings via the sidebar button.
  await page.locator('button[aria-label="Settings"]').click();
  await page.waitForSelector(SETTINGS_DIALOG);

  // General tab is the default active tab — language-select should be visible
  // immediately. If for any reason it isn't, click the General tab explicitly.
  const languageSelect = page.getByTestId('language-select');
  const isVisible = await languageSelect.isVisible().catch(() => false);
  if (!isVisible) {
    await page
      .locator(`${SETTINGS_DIALOG} [role="tab"]`, { hasText: 'General' })
      .click();
    await expect(languageSelect).toBeVisible();
  }

  // Scope the appearance heading to the dialog to avoid ambiguity with any
  // sidebar or other text that might also say "Appearance".
  //
  // Locate the dialog by ROLE, not by its English aria-label: switching the
  // language re-localises the dialog's accessible name (aria-label={$t(
  // 'settings.title')} → "Настройки"), so an aria-label="Settings" selector
  // would go stale the moment the test switches to Russian. There is only one
  // open dialog, so the role match is unambiguous.
  const dialog = page.getByRole('dialog');

  // The language picker is the custom <Select> (button[role=combobox] opening a
  // listbox of role=option), not a native <select>. Drive it by opening the
  // trigger and clicking the option by its visible i18n label. Clicking the
  // option fires mousedown, which is where the Select commits the value.

  // --- English baseline ---
  await languageSelect.click();
  await page.getByRole('option', { name: 'English', exact: true }).click();
  await expect(dialog.getByText('Appearance', { exact: true })).toBeVisible();

  // Plant a sentinel on window to detect whether a reload occurs.
  await page.evaluate(() => {
    (window as Window & { __noReload?: boolean }).__noReload = true;
  });

  // --- Switch to Russian (LOCALE_LABELS['ru'] === 'Русский') ---
  await languageSelect.click();
  await page.getByRole('option', { name: 'Русский', exact: true }).click();

  // The heading must have switched in place without a reload.
  await expect(dialog.getByText('Внешний вид', { exact: true })).toBeVisible();
  await expect(dialog.getByText('Appearance', { exact: true })).toHaveCount(0);

  // Sentinel must still be true — any reload would wipe it.
  const sentinel = await page.evaluate(
    () => (window as Window & { __noReload?: boolean }).__noReload,
  );
  expect(sentinel).toBe(true);
});
