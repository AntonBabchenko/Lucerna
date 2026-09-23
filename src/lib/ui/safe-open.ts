// The single chokepoint for handing a URL to the OS opener — the ONLY file
// under src/ that references `@tauri-apps/plugin-opener` for a URL
// (tools/check-opener-calls.mjs enforces it; docs/PRINCIPLES.md Appendix A).
//
// Threat: several surfaces render URLs that originate in third-party files
// or third-party API fields — a .mrpack / ATLauncher manifest's
// `manual_action_url` (src-tauri modrinth.rs preserves the raw manifest
// download URL verbatim for host_not_allowed entries; atlauncher.rs keeps
// `f.url` verbatim), a pack-completion manifest's `url`
// (pack_completion.rs — "this file is data, never instruction"), Hangar's
// author-supplied `externalUrl` (ModVersion.primary_file.url), and every
// project link a catalogue API returns. Handing such a string to the opener
// with no scheme check would let a crafted pack open `mailto:` / `tel:` /
// cleartext `http:` targets — and, should the capability ACL ever widen,
// `file:` or a custom protocol handler registered by other software.
// Refusing everything but https:// here keeps every link on the one scheme
// those surfaces legitimately need, in one auditable place.
//
// A refusal is never silent (spec 10a §6, fallback discipline): an empty URL
// is a no-op (nothing to open, nothing to say); a non-https URL is refused
// and SAID — an info toast whose selectable line is the URL, so the user can
// copy it and open it by hand if they trust it; a rejected `openUrl` is a
// warning toast carrying the reason. The security stance is unchanged: a
// refused scheme never reaches the opener. The plugin is dynamic-imported
// because it is not resolvable under vitest/SSR.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { pushInfo, pushWarning } from '$lib/toasts/toasts.svelte';

/**
 * Open `url` in the system browser iff it starts with `https://`. Resolves,
 * never throws: a refused scheme is reported as an info toast, a failed open
 * as a warning toast, an empty URL silently does nothing.
 */
export async function openExternalHttps(url: string): Promise<void> {
  if (url === '') return;
  const tr = get(t);
  if (!url.startsWith('https://')) {
    pushInfo(tr('errors.openUrlRefused'), [url]);
    return;
  }
  try {
    const opener = await import('@tauri-apps/plugin-opener');
    await opener.openUrl(url);
  } catch (e) {
    pushWarning(tr('errors.openUrlFailed'), [e instanceof Error ? e.message : String(e)]);
  }
}
