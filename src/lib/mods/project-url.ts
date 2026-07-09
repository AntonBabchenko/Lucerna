// Public web URL for a mod project on its platform. Used by the detail modals
// and the "author disabled third-party download" install error, which all need
// to point the user at the project page to download manually.

import type { ModSource } from '$lib/ipc/bindings';

/**
 * Build the platform project page URL. Prefer the human-readable `slug`; fall
 * back to the raw project id when the slug is unknown (still resolves on both
 * platforms).
 *
 * `author` is optional and only consulted for Hangar: Hangar project URLs are
 * namespaced by owner (`/{author}/{slug}`), but slugs alone still resolve via
 * Hangar's search redirect, so callers that don't have an author on hand
 * (error paths, dependency refs) can omit it.
 */
export function modProjectUrl(source: ModSource, slugOrId: string, author?: string): string {
  // Platform slugs/ids are already url-safe, but encode defensively so a stray
  // character can never break out of the path.
  const seg = encodeURIComponent(slugOrId);
  if (source === 'modrinth') return `https://modrinth.com/mod/${seg}`;
  if (source === 'hangar') {
    return author
      ? `https://hangar.papermc.io/${encodeURIComponent(author)}/${seg}`
      : `https://hangar.papermc.io/${seg}`;
  }
  return `https://www.curseforge.com/minecraft/mc-mods/${seg}`;
}
