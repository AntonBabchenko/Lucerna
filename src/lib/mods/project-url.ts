// Public web URL for a mod project on its platform. Used by the detail modals
// and the "author disabled third-party download" install error, which all need
// to point the user at the project page to download manually.

import type { ModSource } from '$lib/ipc/bindings';

/**
 * Build the platform project page URL. Prefer the human-readable `slug`; fall
 * back to the raw project id when the slug is unknown (still resolves on both
 * platforms).
 */
export function modProjectUrl(source: ModSource, slugOrId: string): string {
  // Platform slugs/ids are already url-safe, but encode defensively so a stray
  // character can never break out of the path.
  const seg = encodeURIComponent(slugOrId);
  return source === 'modrinth'
    ? `https://modrinth.com/mod/${seg}`
    : `https://www.curseforge.com/minecraft/mc-mods/${seg}`;
}
