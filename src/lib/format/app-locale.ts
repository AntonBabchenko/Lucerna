/**
 * The svelte-i18n `locale` store's value, as call sites actually see it.
 *
 * Typed nullable because that is exactly what the store yields before
 * `initLocale()` reconciles the persisted preference; null / undefined fall
 * through to the host default, which is the only thing left at that point.
 *
 * Shared by the formatters in this directory that take the app language
 * explicitly rather than letting a platform API guess it.
 */
export type AppLocale = string | null | undefined;
