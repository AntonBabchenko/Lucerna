import type { AppLocale } from './app-locale';

/**
 * A bare grouped integer in the APP language, e.g. "12,345" / "12 345".
 *
 * This deliberately does NOT live in the dictionary, and that is load-bearing
 * rather than a style preference. A bare count carries no translatable text —
 * only the group separator, which Intl derives from the locale — so its `en`
 * and `ru` values would be byte-identical (`"{n, number}"`). svelte-i18n
 * memoizes message formatters on the message STRING alone: `monadicMemoize`
 * builds its cache key from the first argument only, so the `locale` argument
 * neither reaches the key nor reaches the factory, which then falls back to
 * whatever `getCurrentLocale()` happened to return on the first call. The first
 * locale to render such a message binds its formatter for the rest of the
 * process — open the mod list in English, switch to Russian, and every count
 * would still read "12,345".
 *
 * Every other localized key escapes that trap only because its `en` and `ru`
 * values differ in their unit text ("dl" vs "скач."), which keeps them on
 * separate cache entries. A message with no words has nothing to differ in.
 * Hence a plain Intl call here, taking the locale explicitly, mirroring
 * `./date-time.ts`. `tests/i18n-plural-args.test.ts` guards the dictionary
 * against a future key falling into the same hole.
 */
export function formatCount(locale: AppLocale, n: number): string {
  return new Intl.NumberFormat(locale ?? undefined).format(n);
}
