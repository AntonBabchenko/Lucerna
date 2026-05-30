import { expect, test } from '@playwright/test';
import { installMockIpc } from '../helpers/mock-ipc';
import { setTheme } from '../helpers/theme';

test.skip(process.platform !== 'linux', 'Visual tests pinned to Linux for cross-OS determinism');

// Mirror the Account shape from mock-ipc.ts.
const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

// Mirror the full MockInstance shape (all required fields) from mock-ipc.ts.
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

// The Settings dialog: role="dialog" aria-label="Settings" (SettingsModal.svelte).
// The Settings button in the sidebar: aria-label="Settings" with text "Settings"
// (Sidebar.svelte bottom row).
const SETTINGS_DIALOG = '[role="dialog"][aria-label="Settings"]';

test.describe('Settings modal visual', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`CurseForge panel — ${theme}`, async ({ page }) => {
      await installMockIpc(page, {
        accounts: [offlineAccount],
        active_account_id: 'of-1',
        instances: [baseInstance],
        active_instance_id: 'inst-1',
        theme,
      });
      await page.goto('/');
      await setTheme(page, theme);

      // Open Settings via the sidebar button (aria-label="Settings").
      // The button also carries the visible text "Settings".
      await page.locator('button[aria-label="Settings"]').click();

      // Wait for the dialog to appear before snapshotting.
      await page.waitForSelector(SETTINGS_DIALOG);

      await expect(page.locator(SETTINGS_DIALOG)).toHaveScreenshot(
        `settings-curseforge-${theme}.png`,
      );
    });
  }
});
