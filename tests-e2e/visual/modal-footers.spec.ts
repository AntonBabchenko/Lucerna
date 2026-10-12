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

// ImportedDetailDrawer: role="dialog" aria-label="Imported pack details"
// (ImportedDetailDrawer.svelte).
const IMPORTED_DRAWER = '[role="dialog"][aria-label="Imported pack details"]';

test.describe('Imported modpack drawer footer', () => {
  test('destructive footer: Delete pack LEFT + Open instance RIGHT — light', async ({ page }) => {
    // Pack instance: mrpack_name must be non-null so ImportedView shows a card.
    // mrpack_version and mrpack_source are shown in the drawer header.
    const packInstance = makeInstance({
      id: 'inst-mp-1',
      name: 'Test Pack Instance',
      mrpack_name: 'Test Modpack',
      mrpack_version: '1.0.0',
      mrpack_source: 'modrinth',
      mrpack_project_id: 'abc123',
      mrpack_summary: 'A test modpack for visual snapshots.',
      mrpack_version_id: 'ver-v1',
    });

    await installMockIpc(page, {
      accounts: [offlineAccount],
      active_account_id: 'of-1',
      instances: [packInstance],
      active_instance_id: 'inst-mp-1',
      theme: 'light',
    });
    await page.goto('/');
    await setTheme(page, 'light');

    // Step 1: open the Modpacks modal via the sidebar "Browse modpacks" button.
    await page.locator('button:has-text("Browse modpacks")').click();
    await page.waitForSelector('[data-testid="modpacks-modal"]');

    // Step 2: click the "Imported" sub-tab inside ModpacksTab.
    await page.locator('[role="tab"]:has-text("Imported")').click();

    // Step 3: click the pack card (data-testid="imported-card") to open the drawer.
    await page.locator('[data-testid="imported-card"]').first().click();

    // Step 4: wait for the drawer to mount.
    await page.waitForSelector(IMPORTED_DRAWER);

    await expect(page.locator(IMPORTED_DRAWER)).toHaveScreenshot(
      'imported-drawer-light.png',
    );
  });
});
