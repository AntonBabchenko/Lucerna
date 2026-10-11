// The UI dictionaries in this bundle, discovered from ./locales/*.json — a
// Weblate PR that adds a file is all a new language needs. Kept apart from
// index.ts, which registers them with svelte-i18n: a module that only needs
// the facts (the settings-search registry) must not depend on the store
// module that several tests replace with vi.mock('$lib/i18n').
import {
  canonicalLocaleCode,
  type LocaleInfo,
  localeEndonym,
  SYSTEM_LOCALE_MIN_PERCENT,
  translatedPercent,
} from './catalog';
import { FALLBACK_LOCALE } from './resolve';

export interface BundledLocale extends LocaleInfo {
  dict: unknown;
}

const modules = import.meta.glob('./locales/*.json', { eager: true }) as Record<
  string,
  { default: unknown }
>;

const found = Object.entries(modules)
  .map(([path, mod]) => ({
    code: canonicalLocaleCode(path.match(/\/([^/]+)\.json$/)?.[1] ?? ''),
    dict: mod.default,
  }))
  .filter((l) => l.code !== '')
  .sort((a, b) => (a.code < b.code ? -1 : a.code > b.code ? 1 : 0));

const source = found.find((l) => l.code === FALLBACK_LOCALE)?.dict;

/** Every bundled language, sorted by code, with what the picker needs to know. */
export const BUNDLED_LOCALES: readonly BundledLocale[] = found.map(({ code, dict }) => ({
  code,
  dict,
  endonym: localeEndonym(code),
  percent: translatedPercent(dict, source),
}));

/** Sorted codes that have a dictionary in the bundle. */
export const AVAILABLE_LOCALES: readonly string[] = BUNDLED_LOCALES.map((l) => l.code);

/**
 * The languages "System" may pick on its own: translated far enough that the
 * UI does not come out as a mix with English. Any of the others can still be
 * chosen by hand.
 */
export const SYSTEM_LOCALES: readonly string[] = BUNDLED_LOCALES.filter(
  (l) => l.percent >= SYSTEM_LOCALE_MIN_PERCENT,
).map((l) => l.code);
