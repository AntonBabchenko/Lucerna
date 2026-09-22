// Whether the startup may dial out on its own (the update check, the modpack-update sweep).
//
// In a recovery session the launcher runs on default settings — its `app.json` is in the folder
// it cannot use — so `check_updates_on_startup` reads `true` even for a user who turned it off.
// The restrictive direction is "don't dial": nothing is lost, the manual check in Settings →
// Updates stays available, and the next normal start behaves as configured. "Unknown" (the status
// has not loaded yet) is restrictive for the same reason.

export function startupDialAllowed(
  checkUpdatesOnStartup: boolean,
  statusLoaded: boolean,
  fellBack: boolean,
): boolean {
  return checkUpdatesOnStartup && statusLoaded && !fellBack;
}
