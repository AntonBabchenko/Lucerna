import { expect, test } from '@playwright/test';
import { installMockIpc, makeInstalledMod, makeInstance } from './helpers/mock-ipc';

// Full-app e2e of the MC-version migration flow reached from the Installed tab:
// the compat scan surfaces incompatible mods → the "Fix incompatible mods"
// button opens the migration plan → the plan's fixes and the settled apply
// payload are asserted. Backend is the mock IPC layer (no Rust); the plan is
// injected, so this verifies the FRONTEND behaviour of the recent fixes:
//   #5/#8 a loader-version violation is presented as stranded ("update your
//         loader"), not a reinstall;
//   #2    a checked replaceable force-includes and locks its mandatory
//         new-dependency;
//   partial apply — Apply enables on any selection, and undecided stranded
//         rows are sent as `keep`.

const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

const forgeInstance = makeInstance({
  id: 'inst-1',
  name: 'Forge 1.20.1',
  mc_version: '1.20.1',
  loader: 'forge',
  loader_version: '47.4.10',
});

// A platform build the plan would install / lists as a target. Shape mirrors
// ModVersion in src/lib/ipc/bindings.ts.
function modVersion(projectId: string, versionId: string, name: string) {
  return {
    source: 'modrinth' as const,
    project_id: projectId,
    version_id: versionId,
    name,
    version_number: versionId,
    mc_versions: ['1.20.1'],
    loaders: ['forge'],
    primary_file: {
      filename: `${projectId}-${versionId}.jar`,
      url: 'https://example/mod.jar',
      sha1: `file-${versionId}`,
      size: 1,
      distribution_allowed: true,
      sha256: null,
    },
    deps: [],
    published_at: null,
  };
}

const bopTarget = modVersion('bop', 'v-1211', 'Biomes O Plenty');
const terrablenderTarget = modVersion('terrablender', 'tb-1211', 'TerraBlender');

// Two mods flagged incompatible by the offline scan → incompatibleCount = 2 →
// the "Fix incompatible mods" button appears. Xaero's is a loader-version
// violation; BoP an MC violation.
const compatScan = [
  {
    sha1: 'sha-xaero',
    loader_mismatch: false,
    detected_loader: 'Forge',
    live_checkable: true,
    platform_mismatch: true,
    platform_axis: 'Loader',
    platform_declared: '[52,)',
  },
  {
    sha1: 'sha-bop',
    loader_mismatch: false,
    detected_loader: 'Forge',
    live_checkable: true,
    platform_mismatch: true,
    platform_axis: 'Minecraft',
    platform_declared: '[1.21,)',
  },
];

// The plan the migration dialog renders. Xaero's is STRANDED with the new
// loader_too_old reason (the #5/#8 outcome — never a no-op replaceable). BoP is
// replaceable and mandatorily needs TerraBlender (the #2 coupling).
const migrationPlan = {
  fits: [{ sha1: 'sha-fine', name: 'Fine Mod' }],
  replaceable: [
    { sha1: 'sha-bop', name: 'Biomes O Plenty', source: 'modrinth', project_id: 'bop', target: bopTarget },
  ],
  new_dependencies: [
    { source: 'modrinth', project_id: 'terrablender', target: terrablenderTarget, needed_by: ['Biomes O Plenty'] },
  ],
  stranded: [
    {
      sha1: 'sha-xaero',
      name: "Xaero's Minimap",
      reason: { kind: 'loader_too_old', built_for_mc: '1.21.1' },
    },
    { sha1: 'sha-old', name: 'Old Mod', reason: { kind: 'no_build_for_target' } },
  ],
  unjudged: 0,
};

const installedMods = [
  makeInstalledMod({ sha1: 'sha-xaero', name: "Xaero's Minimap", project_id: 'xaero', filename: 'xaero.jar' }),
  makeInstalledMod({ sha1: 'sha-bop', name: 'Biomes O Plenty', project_id: 'bop', filename: 'bop.jar' }),
  makeInstalledMod({ sha1: 'sha-old', name: 'Old Mod', project_id: 'old', filename: 'old.jar' }),
  makeInstalledMod({ sha1: 'sha-fine', name: 'Fine Mod', project_id: 'fine', filename: 'fine.jar' }),
];

async function openMigrationDialog(page: import('@playwright/test').Page) {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [forgeInstance],
    active_instance_id: 'inst-1',
    installed_mods: installedMods,
    compat_scan: compatScan,
    migration_plan: migrationPlan,
  });
  await page.goto('/');
  await page.getByRole('tab', { name: 'Add-ons' }).click();
  await page.getByRole('tab', { name: 'Installed' }).click();

  // The scan surfaces incompatible mods → the header remediation button appears.
  const fixButton = page.getByRole('button', { name: 'Fix incompatible mods' });
  await expect(fixButton).toBeVisible();
  await fixButton.click();

  // The plan dialog mounts (its replaceable section names the reinstall).
  await expect(page.getByTestId('migration-replaceable-section')).toBeVisible();
}

test('a loader-version violation is stranded with "update your loader", not a reinstall', async ({ page }) => {
  await openMigrationDialog(page);

  // Xaero's is NOT offered as a reinstall — it sits in the stranded section
  // with the loader_too_old copy, and no replaceable row carries its name.
  const stranded = page.getByTestId('migration-stranded-section');
  await expect(stranded).toContainText("Xaero's Minimap");
  // The honest message names the Minecraft version the jar was built for, not a
  // (nonexistent) loader update.
  await expect(stranded).toContainText(/built for minecraft 1\.21\.1/i);
  await expect(page.getByTestId('migration-replaceable-section')).not.toContainText("Xaero's Minimap");
});

test('a checked replaceable force-includes and locks its mandatory dependency', async ({ page }) => {
  await openMigrationDialog(page);

  const depCheckbox = page.getByTestId('migration-new-dep-row-modrinth:terrablender').getByRole('checkbox');
  // Optional until its dependent replace is checked.
  await expect(depCheckbox).not.toBeChecked();
  await expect(depCheckbox).toBeEnabled();

  await page.getByRole('checkbox', { name: 'Biomes O Plenty' }).check();

  // Now forced on and locked.
  await expect(depCheckbox).toBeChecked();
  await expect(depCheckbox).toBeDisabled();
});

test('partial apply enables on any selection and sends keep for undecided stranded + the coupled dependency', async ({
  page,
}) => {
  await openMigrationDialog(page);

  const apply = page.getByTestId('migration-apply-btn');
  // Nothing selected → nothing to apply.
  await expect(apply).toBeDisabled();

  // Only the top-section reinstall; the stranded section is left untouched.
  await page.getByRole('checkbox', { name: 'Biomes O Plenty' }).check();
  await expect(apply).toBeEnabled();
  await apply.click();

  // The settled payload: BoP replaced, TerraBlender rides along (coupling), and
  // BOTH undecided stranded rows are sent as `keep` — never dropped, never a
  // destructive default.
  await expect
    .poll(async () =>
      page.evaluate(() => {
        const w = window as unknown as { __mockIpcCalls?: Array<{ cmd: string; args: unknown }> };
        return (w.__mockIpcCalls ?? []).find((c) => c.cmd === 'mods_apply_mc_migration')?.args ?? null;
      }),
    )
    .toEqual({
      instanceId: 'inst-1',
      selections: {
        replace: [{ old_sha1: 'sha-bop', target: bopTarget }],
        new_dependencies: [terrablenderTarget],
        stranded: [
          { sha1: 'sha-xaero', disposition: 'keep' },
          { sha1: 'sha-old', disposition: 'keep' },
        ],
      },
    });

  // And the result view confirms the apply completed.
  await expect(page.getByTestId('migration-done-btn')).toBeVisible();
});
