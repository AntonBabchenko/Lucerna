import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstance } from '../helpers/mock-ipc';
import { setTheme } from '../helpers/theme';

test.skip(process.platform !== 'linux', 'Visual tests pinned to Linux for cross-OS determinism');

// Mirror the Account shape from mock-ipc.ts — no `kind` field on this branch.
const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

// ready: false so the sidebar shows the install (not Play) state.
const baseInstance = makeInstance({ id: 'inst-1', name: 'Default', ready: false });

// Sidebar renders as <aside> — confirmed in src/lib/layout/Sidebar.svelte.
const SIDEBAR = 'aside';

test.describe('Sidebar visual', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`install state (instance not ready) — ${theme}`, async ({ page }) => {
      await installMockIpc(page, {
        accounts: [offlineAccount],
        active_account_id: 'of-1',
        instances: [{ ...baseInstance, ready: false }],
        active_instance_id: 'inst-1',
        theme,
      });
      await page.goto('/');
      await setTheme(page, theme);
      // The install button label is "↓ Install"; :has-text matches as substring.
      await page.waitForSelector('button:has-text("Install")');
      await expect(page.locator(SIDEBAR).first()).toHaveScreenshot(
        `sidebar-install-${theme}.png`,
      );
    });

    test(`ready state (Play button) — ${theme}`, async ({ page }) => {
      await installMockIpc(page, {
        accounts: [offlineAccount],
        active_account_id: 'of-1',
        instances: [{ ...baseInstance, ready: true }],
        active_instance_id: 'inst-1',
        theme,
      });
      await page.goto('/');
      await setTheme(page, theme);
      await page.waitForSelector('button:has-text("Play")');
      await expect(page.locator(SIDEBAR).first()).toHaveScreenshot(
        `sidebar-play-${theme}.png`,
      );
    });

    test(`running state (Stop button) — ${theme}`, async ({ page }) => {
      await installMockIpc(page, {
        accounts: [offlineAccount],
        active_account_id: 'of-1',
        instances: [{ ...baseInstance, ready: true }],
        active_instance_id: 'inst-1',
        // running shape mirrors Sidebar.svelte prop: { version_id, pid }.
        running: { version_id: '1.20.4', pid: 1234 },
        theme,
      });
      await page.goto('/');
      await setTheme(page, theme);
      await page.waitForSelector('button:has-text("Stop")');
      await expect(page.locator(SIDEBAR).first()).toHaveScreenshot(
        `sidebar-stop-${theme}.png`,
      );
    });
  }
});
