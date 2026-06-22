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

test('language picker switches labels live without a page reload', async ({ page }) => {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [baseInstance],
    active_instance_id: 'inst-1',
  });
  await page.goto('/');

  // Open Settings via the sidebar button. The button exposes its accessible
  // name through its visible text ($t('settings.title') === "Settings"), not an
  // aria-label, so match by role+name rather than a brittle attribute selector.
  await page.getByRole('button', { name: 'Settings', exact: true }).click();

  // Locate the dialog by ROLE, not by a CSS aria-label selector: the modal is
  // labelled via aria-labelledby (the <h2 id="settings-title"> heading), and
  // switching language re-localises that accessible name ("Settings" →
  // "Настройки"). There is only one open dialog, so the role match is
  // unambiguous and survives the language switch.
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // The sidebar Settings button opens the Appearance section, where the language
  // picker lives, so language-select is visible immediately. If for any reason
  // it isn't, click the Appearance section tab explicitly.
  const languageSelect = page.getByTestId('language-select');
  if (!(await languageSelect.isVisible().catch(() => false))) {
    await dialog.getByRole('tab', { name: 'Appearance' }).click();
    await expect(languageSelect).toBeVisible();
  }

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

  // The section label must have switched in place without a reload
  // (settings.sections.appearance: "Appearance" → "Оформление").
  await expect(dialog.getByText('Оформление', { exact: true })).toBeVisible();
  await expect(dialog.getByText('Appearance', { exact: true })).toHaveCount(0);

  // Sentinel must still be true — any reload would wipe it.
  const sentinel = await page.evaluate(
    () => (window as Window & { __noReload?: boolean }).__noReload,
  );
  expect(sentinel).toBe(true);
});
