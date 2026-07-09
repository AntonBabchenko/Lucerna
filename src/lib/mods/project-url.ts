// Public web URL for a mod project on its platform. Used by the detail modals
// and the "author disabled third-party download" install error, which all need
// to point the user at the project page to download manually.

import type { ModSource } from '$lib/ipc/bindings';

/**
 * Build the platform project page URL. Prefer the human-readable `slug`; fall
 * back to the raw project id when the slug is unknown (still resolves on both
 * platforms).
 *
 * `author` is optional and only consulted for Hangar: Hangar project routes
 * REQUIRE the owner segment — `/{author}/{slug}` resolves, but a bare
 * `/{slug}` returns 404 (live-verified: /ViaBackwards 404s while
 * /ViaVersion/ViaBackwards works; there is no slug-only redirect). Callers
 * without an author on hand (error paths, dependency refs) therefore get the
 * Hangar search page pre-queried with the slug — a working page that finds
 * the plugin.
 */
export function modProjectUrl(source: ModSource, slugOrId: string, author?: string): string {
  // Platform slugs/ids are already url-safe, but encode defensively so a stray
  // character can never break out of the path.
  const seg = encodeURIComponent(slugOrId);
  if (source === 'modrinth') return `https://modrinth.com/mod/${seg}`;
  if (source === 'hangar') {
    return author
      ? `https://hangar.papermc.io/${encodeURIComponent(author)}/${seg}`
      : `https://hangar.papermc.io/?query=${seg}`;
  }
  return `https://www.curseforge.com/minecraft/mc-mods/${seg}`;
}
