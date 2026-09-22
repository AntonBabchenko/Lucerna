// Post-update "What's new" prompt. On startup, if the running version differs
// from the last-seen version, show a light action-toast; clicking it opens the
// WhatsNewModal scoped to the versions since `seen`. Fully offline — the
// changelog is embedded, the version comes from the backend. Mirrors the
// update-available toast pattern in +page.svelte.
//
// Rune-state-in-a-.svelte.ts module — the same idiom as
// `$lib/toasts/toasts.svelte` and `$lib/update/state.svelte`.
import { get } from 'svelte/store';
import { recoverySessionOrUnknown } from '$lib/settings/data-location.svelte';
import { locale, t } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import { dismiss, pushActionToast } from '$lib/toasts/toasts.svelte';
import { CHANGELOG_SOURCE_LOCALE } from './locales';
import { changelogSince, hasRenderableEntry } from './since';
import { CHANGELOG } from './source';
import { ensureChangelogTranslation } from './translation.svelte';
import type { Changelog, ChangelogVersion } from './types';

/** Non-null `entries` means the modal is open, showing these versions. */
export const whatsNewState = $state<{ entries: ChangelogVersion[] | null }>({ entries: null });

/** Auto-hide the prompt after this long. It is marked seen on show, so hiding
 *  is purely cosmetic — it never reappears for the same version. */
const WHATS_NEW_TOAST_TTL_MS = 8000;

/** Injectable dependencies (tests pass fakes); production uses the backend. */
export interface WhatsNewDeps {
  entries?: Changelog;
  currentVersion?: () => Promise<string>;
  markSeen?: (version: string) => Promise<void>;
  /** Recovery session, or could not tell — either means "not now". */
  recoverySession?: () => Promise<boolean>;
}

/** Best-effort persist — a failure just means the prompt may show again next
 *  launch, which is acceptable. */
async function persistSeen(version: string): Promise<void> {
  try {
    await commands.changelogMarkSeen(version);
  } catch {
    /* best-effort */
  }
}

/**
 * Decide whether to prompt, and prompt. Fire-and-forget from onMount; any
 * failure to read the version is swallowed (we simply don't prompt).
 */
export async function checkWhatsNew(seen: string | null, deps: WhatsNewDeps = {}): Promise<void> {
  const entries = deps.entries ?? CHANGELOG;
  const currentVersion = deps.currentVersion ?? (() => commands.appVersion());
  const markSeen = deps.markSeen ?? persistSeen;
  const recoverySession = deps.recoverySession ?? recoverySessionOrUnknown;

  // A recovery session runs on a throwaway root: the baseline write below would land in an
  // app.json that is deleted at exit, and a prompt over the recovery banner is noise. "Could not
  // tell" is treated the same way — it is offered again on the next start.
  try {
    if (await recoverySession()) return;
  } catch {
    return;
  }

  let current: string;
  try {
    current = await currentVersion();
  } catch {
    return; // can't tell what version we are — do nothing
  }

  const since = changelogSince(entries, current, seen);
  if (!hasRenderableEntry(since)) {
    // Nothing to show. Still advance the baseline (unless already current) so a
    // later real gap is scoped from here, not from a stale value.
    if (seen !== current) void markSeen(current);
    return;
  }

  // Mark seen up front → once per version, never nags (mirrors update_dismiss).
  void markSeen(current);

  // Start loading the UI language's changelog now, while the toast is on
  // screen, so the dialog opens already translated instead of flashing
  // English first. A no-op for English and for a locale already loaded.
  void ensureChangelogTranslation(get(locale) ?? CHANGELOG_SOURCE_LOCALE);

  const tr = get(t);
  const toastId = pushActionToast('info', tr('page.whatsNew.toast', { version: current }), {
    label: tr('page.whatsNew.actionLabel'),
    run: () => {
      whatsNewState.entries = since;
    },
  });
  setTimeout(() => dismiss(toastId), WHATS_NEW_TOAST_TTL_MS);
}
