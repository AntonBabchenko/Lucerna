// Smoke coverage for the top-level Client/Servers mode switcher (ModeSwitcher +
// +page.svelte's two mounted-but-hidden panels). Exercises:
//   - the switcher's aria-pressed state
//   - both panels staying mounted across a switch (the client tab survives)
//   - the cascade regression: the [hidden] attribute loses to .flex in
//     Tailwind v3, so the inactive panel wrapper must carry the `hidden`
//     CLASS — asserted via the panel-client/panel-servers testids added to
//     +page.svelte for exactly this purpose
//   - reconcile() auto-selecting the only server so the management tabs (not
//     the empty hero) render on entering servers mode

import { expect, test, type Locator } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';
import type { ServerWithStatus_Serialize } from '../src/lib/ipc/bindings';

// Precise check for the `hidden` utility CLASS (not the `hidden` attribute —
// see the [hidden]-loses-to-.flex comment in +page.svelte). A substring/regex
// match against className (e.g. /hidden/) false-positives on the sibling
// `overflow-hidden` utility class both wrapper divs always carry, so this
// reads classList directly. Wrapped in expect.poll for the same auto-retry
// behavior toHaveClass would give.
async function expectHiddenClass(locator: Locator, expected: boolean): Promise<void> {
  await expect
    .poll(() => locator.evaluate((el) => el.classList.contains('hidden')))
    .toBe(expected);
}

// Field list mirrors makeServer() in tests/server-state.test.ts — every field
// ServerWithStatus_Serialize requires, so the UI (sidebar Start button, status
// pill, management tabs) mounts without throwing on a missing property.
function makeServer(overrides: Partial<ServerWithStatus_Serialize> = {}): ServerWithStatus_Serialize {
  return {
    id: 'srv-1',
    name: 'Test Server',
    mc_version: '1.21',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running: false,
    pid: null,
    port: 25565,
    upload: null,
    upload_password_set: false,
    last_exit_code: null,
    diagnosis_status: 'none',
    ...overrides,
  };
}

test('mode switcher swaps panels and preserves the client tab', async ({ page }) => {
  await installMockIpc(page, { instances: [makeInstance()], active_instance_id: 'inst-1' });
  await page.goto('/');

  // Client mode by default. First assertion after goto doubles as the
  // cold-boot wait: a cold Vite server's on-demand transform storm exceeds
  // the default 5s expect timeout; the rest run against a rendered app.
  await expect(page.getByTestId('mode-switch-client')).toHaveAttribute('aria-pressed', 'true', {
    timeout: 15_000,
  });

  // Move the client off its default tab (nav.worlds === "Worlds" in en.json).
  await page.getByRole('tab', { name: 'Worlds' }).click();
  await expect(page.getByRole('tab', { name: 'Worlds' })).toHaveAttribute('aria-selected', 'true');

  // Servers mode: empty hero + sidebar create; switcher state flips.
  await page.getByTestId('mode-switch-servers').click();
  await expect(page.getByTestId('servers-empty-hero')).toBeVisible();
  await expect(page.getByTestId('sidebar-create-server')).toBeVisible();
  await expect(page.getByTestId('mode-switch-servers')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('mode-switch-client')).toHaveAttribute('aria-pressed', 'false');

  // Cascade regression ([hidden] loses to .flex): the inactive panel wrapper must carry the
  // `hidden` CLASS, and the client tablist must not be visible while hidden.
  // Use a plain [role] CSS locator (not getByRole) — Playwright's role
  // locator excludes accessibility-hidden subtrees by default, which would
  // make this assertion vacuously true even if the `hidden` class were lost.
  const clientPanel = page.getByTestId('panel-client');
  await expectHiddenClass(clientPanel, true);
  await expect(clientPanel.locator('[role="tablist"]')).not.toBeVisible();
  await expectHiddenClass(page.getByTestId('panel-servers'), false);

  // Back to client: the Worlds tab is still the active one (panel stayed
  // mounted — the mode switch didn't remount MainTabs / reset its state).
  await page.getByTestId('mode-switch-client').click();
  await expect(page.getByRole('tab', { name: 'Worlds' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('servers-empty-hero')).not.toBeVisible();
  await expectHiddenClass(page.getByTestId('panel-servers'), true);
  await expectHiddenClass(page.getByTestId('panel-client'), false);
});

test('entering servers mode with one server auto-selects it and shows the management tabs', async ({
  page,
}) => {
  await installMockIpc(page, {
    instances: [makeInstance()],
    active_instance_id: 'inst-1',
    servers: [makeServer()],
  });
  await page.goto('/');

  // First assertion after goto doubles as the cold-boot wait (see test 1).
  await expect(page.getByTestId('mode-switch-servers')).toBeVisible({ timeout: 15_000 });
  await page.getByTestId('mode-switch-servers').click();

  // reconcile() auto-selected the only server — no empty hero, sidebar Start
  // (not sidebar-create-server's hero counterpart) is visible instead.
  await expect(page.getByTestId('servers-empty-hero')).not.toBeVisible();
  await expect(page.getByTestId('sidebar-server-start')).toBeVisible();

  // The 8 server-management tabs (console/connect/general/settings/mods/
  // plugins/hosting/backups) render inside the servers panel.
  const managementTabs = page.getByTestId('panel-servers').getByRole('tab');
  await expect(managementTabs).toHaveCount(8);
});
