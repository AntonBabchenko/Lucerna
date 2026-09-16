// The changelog translation for each UI locale, loaded once per session and
// shared by every ChangelogPanel (Settings → Updates and the post-update
// dialog). Rune-state-in-a-.svelte.ts module — the idiom of
// `$lib/toasts/toasts.svelte` and `./whats-new.svelte`.
import { CHANGELOG_SOURCE_LOCALE, type LocaleLoad, loadChangelogLocale } from './locales';

/** Completed loads keyed by locale code. A missing key means "not requested
 *  yet or still loading" — the panel shows English without a note meanwhile,
 *  since nothing is known yet and so nothing is claimed. */
export const changelogTranslations = $state<Record<string, LocaleLoad>>({});

const inFlight = new Map<string, Promise<void>>();

/**
 * Make sure `locale`'s changelog is loaded, or known to be missing or broken.
 * The source locale needs nothing. Concurrent callers share one load.
 */
export function ensureChangelogTranslation(locale: string): Promise<void> {
  if (locale === CHANGELOG_SOURCE_LOCALE || locale in changelogTranslations) {
    return Promise.resolve();
  }
  let pending = inFlight.get(locale);
  if (!pending) {
    pending = loadChangelogLocale(locale)
      // loadChangelogLocale never rejects by contract; if that contract ever
      // breaks, the outcome is still recorded as a failure rather than leaving
      // the locale "loading" forever (English shown with no note).
      .catch((error: unknown): LocaleLoad => ({ status: 'failed', error }))
      .then((load) => {
        if (load.status === 'failed') {
          // The panel tells the user; the detail is for whoever debugs it.
          console.warn(`[changelog] could not load the "${locale}" translation:`, load.error);
        }
        changelogTranslations[locale] = load;
      })
      .finally(() => inFlight.delete(locale));
    inFlight.set(locale, pending);
  }
  return pending;
}
