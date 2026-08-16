/**
 * Date / time rendering that follows the APP language, not the host OS.
 *
 * `Date.prototype.toLocale*String()` called with no locale argument silently
 * uses the OS locale, so a Russian UI on an English Windows printed
 * "8/13/2026, 9:41 PM" beside Russian copy — and switching the in-app language
 * moved nothing. Every caller passes the svelte-i18n `locale` store's value
 * (`$locale` in a component, `get(locale)` in a plain module), so the rendered
 * date tracks the language switch like the rest of the UI.
 *
 * `locale` is typed nullable because that is exactly what the store yields
 * before `initLocale()` reconciles the persisted preference; null / undefined
 * fall through to the host default — the only thing left, and the same shape
 * the `$locale ?? undefined` call sites already used.
 *
 * These stay thin wrappers on purpose. The value is a single grep-able home
 * for "did this date get a locale?" (enforced by tests/no-os-locale-dates.test.ts),
 * not a new formatting policy: copy AROUND a date still belongs in the
 * dictionary, as it already does for `instance.integrity.checkedAt` and
 * `modpacks.switch.versionMeta`.
 *
 * Number formatting is deliberately NOT here: a numeric `toLocaleString()` is
 * the wrong tool entirely — the dictionary types the argument `{n, number}`
 * and the caller passes a raw number (see `$lib/format/size.ts`).
 */

/** The svelte-i18n locale store's value: a BCP-47 tag, or null before init. */
export type AppLocale = string | null | undefined;

/** Date + time of day, e.g. "8/13/2026, 9:41 PM" / "13.08.2026, 21:41". */
export function formatDateTime(
  locale: AppLocale,
  ms: number,
  options?: Intl.DateTimeFormatOptions,
): string {
  return new Date(ms).toLocaleString(locale ?? undefined, options);
}

/** Calendar date only, e.g. "8/13/2026" / "13.08.2026". */
export function formatDate(
  locale: AppLocale,
  ms: number,
  options?: Intl.DateTimeFormatOptions,
): string {
  return new Date(ms).toLocaleDateString(locale ?? undefined, options);
}

/** Time of day only, e.g. "9:41 PM" / "21:41". */
export function formatTimeOfDay(
  locale: AppLocale,
  ms: number,
  options?: Intl.DateTimeFormatOptions,
): string {
  return new Date(ms).toLocaleTimeString(locale ?? undefined, options);
}
