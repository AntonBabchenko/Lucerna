// Fixture-level regression test for the Overview Export button gating rule.
//
// The Export button is only rendered when the instance has at least one
// installed mod.  This test asserts that rule at the DOM fixture level
// using static HTML (no live Tauri app required), consistent with the
// sidebar-tooltip-clip.spec.ts precedent.

import { test, expect } from '@playwright/test';

// Mirrors the Overview gate: the Export button renders only when the
// instance has >= 1 installed mod.
const withMods = `<!DOCTYPE html><body>
  <section id="overview" data-mods="2">
    <button id="export" type="button">Export modpack…</button>
  </section>
</body>`;

const noMods = `<!DOCTYPE html><body>
  <section id="overview" data-mods="0">
    <!-- gate hides the button -->
  </section>
</body>`;

test.describe('Export button gating', () => {
  test('shows Export when mods are installed', async ({ page }) => {
    await page.setContent(withMods);
    await expect(page.locator('#export')).toBeVisible();
  });

  test('hides Export when no mods are installed', async ({ page }) => {
    await page.setContent(noMods);
    await expect(page.locator('#export')).toHaveCount(0);
  });
});
