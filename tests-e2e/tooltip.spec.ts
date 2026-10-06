// Real-browser checks for the shared tooltip (src/lib/ui/tooltip/). happy-dom
// (the `pnpm test` env) has no layout engine and no real focus/hover timing, so
// the action's user-facing loop — hover-to-show (with delay), focus-to-show,
// Escape-to-hide — and where the bubble lands are verified here against
// headless Chromium driving the real app (mock IPC, no Rust backend).
//
// Target of the first block: the Sidebar compact toggle. It is always present
// (top of the sidebar), carries `use:tooltip`, and in the default (non-compact)
// state its accessible name + tooltip text is "Collapse to mini mode"
// (src/lib/layout/Sidebar.svelte; i18n key sidebar.compactCollapse).

import { expect, type Locator, type Page, test } from '@playwright/test';
// Pure (no $lib, no runes), so the spec imports the placement rule itself.
import { computePosition } from '../src/lib/ui/tooltip/position';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

const TOGGLE_NAME = /collapse to mini mode|expand to full window/i;

// An uncaught error stops Svelte's effects — TooltipLayer's among them — so a
// run that threw proves nothing about the tooltip (docs/UI-TESTING.md, "An
// uncaught error inside an effect …"). Every test here fails on one.
let errors: string[] = [];

test.beforeEach(({ page }) => {
  errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
});

test.afterEach(() => {
  expect(errors, 'uncaught errors in the page').toEqual([]);
});

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

// Where the bubble lands, driven by real input only: Playwright's keyboard and
// mouse, so Chromium decides :focus-visible and lays the page out — synthetic
// events reproduce neither. Each showing starts at the window's origin, out of
// sight, and TooltipLayer's $effect measures and places it. In e2e that effect
// once never ran — the mock threw inside an effect at boot, and a dialog's body
// threw again — and every tooltip in a dialog sat in the top-left corner.

const OPEN_MODPACKS = { name: 'Browse modpacks' } as const;
const MODPACKS_HELP = { name: 'Why does installing a modpack create a new instance?' } as const;
const CLOSE_MODPACKS = { name: 'Close modpacks' } as const;

// The bubble is in sight, shows `text`, and stands where the placement rule
// puts it for this trigger and the bubble's own size — above it, flipped below
// without room, centred unless clamped to the window. All the triggers here
// ask for the default, 'top'. Polled: the entrance transition moves the box
// for 120 ms.
async function expectPlacedBy(page: Page, trigger: Locator, text: string) {
  const bubble = page.locator('#app-tooltip');
  await expect(bubble).toBeVisible();
  await expect(bubble).toHaveText(text);
  await expect
    .poll(async () => {
      const t = await trigger.boundingBox();
      const b = await bubble.boundingBox();
      const viewport = page.viewportSize();
      if (!t || !b || !viewport) return 'not laid out';
      // The size TooltipLayer measures: its layout box, untouched by the transition.
      const size = await bubble.evaluate((el: HTMLElement) => ({
        width: el.offsetWidth,
        height: el.offsetHeight,
      }));
      const rect = {
        top: t.y,
        left: t.x,
        width: t.width,
        height: t.height,
        bottom: t.y + t.height,
      };
      const want = computePosition(rect, size, 'top', viewport);
      if (Math.abs(b.x - want.left) < 1 && Math.abs(b.y - want.top) < 1) {
        return 'placed by its trigger';
      }
      const at = (x: number, y: number) => `${Math.round(x)},${Math.round(y)}`;
      return `bubble at ${at(b.x, b.y)}, its place ${at(want.left, want.top)}`;
    })
    .toBe('placed by its trigger');
}

// Real Tab presses until `target` has the focus: the focus arrives the way a
// keyboard user's does, and a control added earlier in the tab order does not
// break the spec.
async function tabTo(page: Page, target: Locator, max = 20) {
  for (let i = 0; i < max; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return;
    await page.keyboard.press('Tab');
  }
  await expect(target).toBeFocused();
}

test.describe('shared tooltip placement', () => {
  test.beforeEach(async ({ page }) => {
    await installMockIpc(page, {
      instances: [makeInstance({ id: 'inst-1', name: 'Vanilla' })],
      active_instance_id: 'inst-1',
    });
    await page.goto('/');
    await expect(page.getByRole('button', OPEN_MODPACKS)).toBeVisible({ timeout: 15_000 });
  });

  test('inside a dialog, Tab and Shift+Tab show each tooltip by its control', async ({ page }) => {
    // The session's first showing happens inside the dialog, as reported: the
    // mouse opens it, and nothing on the page has shown a tooltip yet.
    await page.getByRole('button', OPEN_MODPACKS).click();
    const dialog = page.getByRole('dialog', { name: 'Modpacks' });
    await expect(dialog.getByRole('button', MODPACKS_HELP)).toBeFocused();

    await page.keyboard.press('Tab');
    const close = dialog.getByRole('button', CLOSE_MODPACKS);
    await expect(close).toBeFocused();
    await expectPlacedBy(page, close, 'Close modpacks');

    await page.keyboard.press('Shift+Tab');
    const help = dialog.getByRole('button', MODPACKS_HELP);
    await expect(help).toBeFocused();
    await expectPlacedBy(page, help, 'Why a new instance?');
  });

  test('a dialog opened from the keyboard shows its first control tooltip by it', async ({
    page,
  }) => {
    await tabTo(page, page.getByRole('button', OPEN_MODPACKS));
    await page.keyboard.press('Enter');
    // The focus trap's initial focus follows a key press, so it is :focus-visible.
    const help = page.getByRole('dialog', { name: 'Modpacks' }).getByRole('button', MODPACKS_HELP);
    await expect(help).toBeFocused();
    await expectPlacedBy(page, help, 'Why a new instance?');
  });

  test('in another dialog, keyboard focus shows the tooltip by its control', async ({ page }) => {
    await page.getByRole('button', { name: 'Manage', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Manage Instances' });
    await expect(dialog).toBeVisible();

    const picture = dialog.getByRole('button', { name: 'Change instance picture' });
    await tabTo(page, picture);
    await expectPlacedBy(page, picture, 'Change instance picture');
  });

  test('inside a dialog, hover shows the tooltip by its control', async ({ page }) => {
    await page.getByRole('button', OPEN_MODPACKS).click();
    const close = page
      .getByRole('dialog', { name: 'Modpacks' })
      .getByRole('button', CLOSE_MODPACKS);
    await close.hover();
    await expectPlacedBy(page, close, 'Close modpacks');
  });

  test('outside any dialog, keyboard focus shows the tooltip by its control', async ({ page }) => {
    const toggle = page.getByRole('button', { name: 'Collapse to mini mode' });
    await tabTo(page, toggle);
    await expectPlacedBy(page, toggle, 'Collapse to mini mode');
  });
});
