// The single chokepoint for handing a URL to the OS opener.
//
// Threat: several surfaces render URLs that originate in third-party files
// or third-party API fields — a .mrpack / ATLauncher manifest's
// `manual_action_url` (src-tauri modrinth.rs preserves the raw manifest
// download URL verbatim for host_not_allowed entries; atlauncher.rs keeps
// `f.url` verbatim), a pack-completion manifest's `url`
// (pack_completion.rs — "this file is data, never instruction"), and
// Hangar's author-supplied `externalUrl` (ModVersion.primary_file.url).
// Handing such a string to the opener with no scheme check would let a
// crafted pack open `mailto:` / `tel:` / cleartext `http:` targets — and,
// should the capability ACL ever widen, `file:` or a custom protocol
// handler registered by other software. Refusing everything but https://
// here keeps every remote-data link on the one scheme those surfaces
// legitimately need, in one auditable place.
//
// Non-https is a silent no-op, mirroring the two pre-existing in-tree
// guards (ChangelogPanel.openUrl, FixModRepairCard.openProject). The
// plugin is dynamic-imported for the same reason every call site did it
// inline: it is not resolvable under vitest/SSR.

/** Open `url` in the system browser iff it starts with `https://`; no-op otherwise. */
export async function openExternalHttps(url: string): Promise<void> {
  if (!url.startsWith('https://')) return;
  const opener = await import('@tauri-apps/plugin-opener');
  await opener.openUrl(url);
}
