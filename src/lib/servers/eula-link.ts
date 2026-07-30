/**
 * The Minecraft EULA the launcher asks the user to accept before a server can be
 * created, imported, or repaired.
 *
 * The URL is Mojang's canonical short link and is deliberately the same one the
 * Rust side writes into `runtime/eula.txt`
 * (`src-tauri/src/servers_runtime/eula.rs`) — the user must be able to read the
 * exact document they are agreeing to.
 *
 * We link to it rather than bundling the text: the EULA is Microsoft's document
 * and changes without our involvement, so an embedded copy would eventually
 * present a stale revision as the agreement in force.
 */
export const EULA_URL = 'https://aka.ms/MinecraftEULA';

/** Open the EULA in the user's browser. Side effect isolated for testability. */
export function openEula(): void {
  void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(EULA_URL));
}
