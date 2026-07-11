// Real-browser checks for the shared tooltip (src/lib/ui/tooltip/). happy-dom
// (the `pnpm test` env) has no layout engine and no real focus/hover timing, so
// the action's user-facing loop — hover-to-show (with delay), focus-to-show,
// Escape-to-hide — is verified here against headless Chromium driving the real
// app (mock IPC, no Rust backend).
//
// Target: the Sidebar compact toggle. It is always present (top of the sidebar),
// carries `use:tooltip`, and in the default (non-compact) state its accessible
// name + tooltip text is "Collapse to mini mode" (src/lib/layout/Sidebar.svelte;
// i18n key sidebar.compactCollapse).

import { expect, test } from '@playwright/test';
import { installMockIpc } from './helpers/mock-ipc';

const TOGGLE_NAME = /collapse to mini mode|expand to full window/i;

test.describe('shared tooltip', () => {
  test('appears on hover of an icon control and carries its text', async ({ page }) => {
    await installMockIpc(page);
    await page.goto('/');

    const toggle = page.getByRole('button', { name: TOGGLE_NAME }).first();
    // First assertion after goto doubles as the cold-boot wait: a cold Vite
    // server's on-demand transform storm exceeds the default 5s expect timeout.
    await expect(toggle).toBeVisible({ timeout: 15_000 });
    await toggle.hover();

    const bubble = page.locator('#app-tooltip');
    // Hover has a ~400ms open delay; allow generous time for it to appear.
    await expect(bubble).toBeVisible({ timeout: 2000 });
    await expect(bubble).toHaveText(/.+/);
  });

  test('appears immediately on keyboard focus and hides on Escape', async ({ page }) => {
    await installMockIpc(page);
    await page.goto('/');

    const toggle = page.getByRole('button', { name: TOGGLE_NAME }).first();
    await toggle.focus();
    await expect(page.locator('#app-tooltip')).toBeVisible({ timeout: 2000 });

    await page.keyboard.press('Escape');
    await expect(page.locator('#app-tooltip')).toBeHidden();
  });
});
