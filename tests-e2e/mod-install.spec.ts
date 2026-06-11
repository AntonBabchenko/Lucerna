import { expect, test } from '@playwright/test';
import { installMockIpc } from './helpers/mock-ipc';

// Full-app e2e of the core "browse → install a mod → card shows Installed"
// flow, driven against the mock IPC layer (no Rust backend). Pairs with the
// component-level coverage of ModBrowseView/ModCard; this exercises the real
// rendered app end-to-end: Add-ons tab → mod browser → install → installed
// state, including the page-level wiring that unit tests stub out.
//
// The instance is pre-seeded (a fabric instance, so the mod browser is usable
// rather than showing the vanilla notice). Instance creation via the Manage
// modal is covered by the component tests; seeding here keeps the e2e focused
// on the install path and free of modal-form flakiness.

const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

const fabricInstance = {
  id: 'inst-1',
  name: 'Fabric 1.20.4',
  mc_version: '1.20.4',
  loader: 'fabric' as const,
  loader_version: '0.15.0',
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

const sodium = {
  source: 'modrinth' as const,
  project_id: 'sodium',
  slug: 'sodium',
  name: 'Sodium',
  summary: 'A modern rendering engine.',
  icon_url: null,
  downloads: 1_000_000,
  author: 'CaffeineMC',
  updated_at: null,
};

test('install a mod from the browser → card shows Installed', async ({ page }) => {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [fabricInstance],
    active_instance_id: 'inst-1',
    mod_hits: [sodium],
  });
  await page.goto('/');

  // Open the Add-ons tab — it defaults to the mod browser (kind=mod, view=browse).
  await page.getByRole('tab', { name: 'Add-ons' }).click();

  // The search runs on mount; the seeded hit renders as a card with an Install
  // button. (Type into the search box too, to exercise the search path.)
  await page.getByLabel('Search mods').fill('sodium');

  const installButton = page.getByRole('button', { name: 'Install', exact: true });
  await expect(installButton).toBeVisible();
  await installButton.click();

  // After the fast-path install (no deps, loader matches), the browse view
  // refreshes its installed list and the card flips to the "Installed · vX"
  // pill (the "·" disambiguates it from the "Installed" sub-tab and the name).
  await expect(page.getByText(/Installed ·/)).toBeVisible();
  // The Install button is gone — the card no longer offers to install.
  await expect(page.getByRole('button', { name: 'Install', exact: true })).toHaveCount(0);
});
