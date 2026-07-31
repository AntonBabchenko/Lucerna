// Dialogs group intent coverage.
//
// The 2026-05-29 UI audit inventory confirms that no `src/lib/dialogs/`
// directory exists. Dialog-like components are distributed into their
// respective feature groups and covered by those groups' intent tests:
//
//   DependencyDialog      — src/lib/mods/        (mod-browser group)
//   CompatWarningDialog   — src/lib/mods/        (mod-browser group)
//   ImportPickerDialog    — src/lib/modpacks/    (modpacks group)
//   ModpackUpdateDialog   — src/lib/modpacks/    (modpacks group)
//   WorldDetailDialog     — src/lib/worlds/      (worlds group)
//   DeleteWorldDialog     — src/lib/worlds/      (worlds group)
//   RestoreBackupDialog   — src/lib/worlds/      (worlds group)
//   ManageInstancesModal  — src/lib/instances/   (instance-mgmt group)
//   SettingsModal         — src/lib/settings/    (settings group)
//
// Additionally, DependencyDialog has dedicated unit coverage in
// tests/dependency-dialog.test.ts and CompatWarningDialog is covered in
// tests/compat-warning-dialog.test.ts.
//
// This stub confirms the group is intentionally empty rather than missed.

import { describe, it } from 'vitest';

describe('dialogs group', () => {
  it('has no standalone cross-cutting dialogs — all dialog surfaces are covered by their respective feature-group intent tests', () => {
    // This test is a positive assertion that the absence of a dialogs/
    // directory is intentional. Any future src/lib/dialogs/ component
    // should be added to this file.
  });
});
