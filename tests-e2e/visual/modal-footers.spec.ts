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

// Base instance shape (all required fields from MockInstance in mock-ipc.ts).
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

// ManageInstancesModal: inner div identified by the h2 "Manage Instances" heading.
// DOM tree: fixed-backdrop > div.bg-surface > header > h2
// xpath=../.. from h2 reaches the bg-surface inner modal div.
const MANAGE_H2 = 'h2:has-text("Manage Instances")';
const MANAGE_MODAL = `xpath=h2[contains(text(),'Manage Instances')]/../../..`;

// ImportedDetailDrawer: role="dialog" aria-label="Imported pack details"
// (ImportedDetailDrawer.svelte).
const IMPORTED_DRAWER = '[role="dialog"][aria-label="Imported pack details"]';

test.describe('Manage instances modal footer', () => {
  test('destructive footer: Delete LEFT + Done RIGHT — light', async ({ page }) => {
    // Two instances are required so the Delete button is enabled
    // (instances.length <= 1 disables it — ManageInstancesModal.svelte line ~413).
    await installMockIpc(page, {
      accounts: [offlineAccount],
      active_account_id: 'of-1',
      instances: [
        baseInstance,
        { ...baseInstance, id: 'inst-2', name: 'Second' },
      ],
      active_instance_id: 'inst-1',
      theme: 'light',
    });
    await page.goto('/');
    await setTheme(page, 'light');

    // Open the Manage modal via the sidebar "⚙ Manage" button.
    await page.locator('button:has-text("Manage")').first().click();

    // Wait for the modal heading to appear before snapshotting.
    await page.waitForSelector(MANAGE_H2);

    // Snapshot the inner modal div (two levels above the h2 heading:
    // h2 < header < div.bg-surface).
    const h2 = page.locator(MANAGE_H2).first();
    const innerModal = h2.locator('xpath=../../..');

    await expect(innerModal).toHaveScreenshot('manage-modal-light.png');
  });
});

test.describe('Imported modpack drawer footer', () => {
  test('destructive footer: Delete pack LEFT + Open instance RIGHT — light', async ({ page }) => {
    // Pack instance: mrpack_name must be non-null so ImportedView shows a card.
    // mrpack_version and mrpack_source are shown in the drawer header.
    const packInstance = {
      ...baseInstance,
      id: 'inst-mp-1',
      name: 'Test Pack Instance',
      mrpack_name: 'Test Modpack',
      mrpack_version: '1.0.0',
      mrpack_source: 'modrinth' as const,
      mrpack_project_id: 'abc123',
      mrpack_summary: 'A test modpack for visual snapshots.',
      mrpack_version_id: 'ver-v1',
    };

    await installMockIpc(page, {
      accounts: [offlineAccount],
      active_account_id: 'of-1',
      instances: [packInstance],
      active_instance_id: 'inst-mp-1',
      theme: 'light',
    });
    await page.goto('/');
    await setTheme(page, 'light');

    // Step 1: open the Modpacks view via the sidebar "Browse modpacks" button.
    await page.locator('button:has-text("Browse modpacks")').click();

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
