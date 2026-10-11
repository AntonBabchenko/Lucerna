import type { AppLocale } from './app-locale';

/**
 * A bare grouped integer in the APP language, e.g. "12,345" / "12 345".
 *
 * This deliberately does NOT live in the dictionary. A bare count carries no
 * translatable text — only the group separator, which Intl derives from the
 * locale — so its `en` and `ru` values would be byte-identical
 * (`"{n, number}"`). It moved out because svelte-i18n memoizes message
 * formatters on the message STRING alone: `monadicMemoize` builds its cache
 * key from the first argument only, so the first locale to render such a
 * message bound its formatter for the rest of the process — open the mod list
 * in English, switch to Russian, and every count still read "12,345".
 *
 * `$t` no longer formats through that cache: `$lib/i18n/typed.ts` keeps its
 * own, keyed by locale AND message (a partial community translation shares
 * every untranslated message with English, so avoiding identical strings
 * stopped being possible). A message with no words still has no business in a
 * dictionary translators edit, so this stays a plain Intl call taking the
 * locale explicitly, mirroring `./date-time.ts`.
 */
export function formatCount(locale: AppLocale, n: number): string {
  return new Intl.NumberFormat(locale ?? undefined).format(n);
}
