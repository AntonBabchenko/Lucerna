import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstance } from '../helpers/mock-ipc';
import { setTheme } from '../helpers/theme';

test.skip(process.platform !== 'linux', 'Visual tests pinned to Linux for cross-OS determinism');

// Mirror the Account shape from mock-ipc.ts.
const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

const baseInstance = makeInstance({ id: 'inst-1', name: 'Default' });

// The sidebar Settings button opens the dialog on its first tab, Appearance.
// Both are found by ROLE + accessible name (the dialog is labelled through
// aria-labelledby, the button through its visible text — neither carries an
// aria-label a CSS selector could match), exactly as settings-search.spec.ts.
test.describe('Settings modal visual', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`Appearance panel — ${theme}`, async ({ page }) => {
      await installMockIpc(page, {
        accounts: [offlineAccount],
        active_account_id: 'of-1',
        instances: [baseInstance],
        active_instance_id: 'inst-1',
        theme,
      });
      await page.goto('/');
      await setTheme(page, theme);

      await page.getByRole('button', { name: 'Settings', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'Settings' });
      await expect(dialog).toBeVisible();
      await expect(dialog.getByRole('tab', { name: 'Appearance', selected: true })).toBeVisible();

      await expect(dialog).toHaveScreenshot(`settings-appearance-${theme}.png`);
    });
  }
});
