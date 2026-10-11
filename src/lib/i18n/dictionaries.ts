// The UI dictionaries in this bundle, discovered from ./locales/*.json — a
// Weblate PR that adds a file is all a new language needs. Kept apart from
// index.ts, which registers them with svelte-i18n: a module that only needs
// the facts (the settings-search registry) must not depend on the store
// module that several tests replace with vi.mock('$lib/i18n'). The logic is
// catalog.ts's (pure, tested); this module only feeds it the glob.
import { type BundledLocale, bundleLocales, systemLocaleCodes } from './catalog';
import { FALLBACK_LOCALE } from './resolve';

/** Every bundled language, sorted by code, with what the picker needs to know. */
export const BUNDLED_LOCALES: readonly BundledLocale[] = bundleLocales(
  import.meta.glob('./locales/*.json', { eager: true }) as Record<string, { default: unknown }>,
  FALLBACK_LOCALE,
);

/** Sorted codes that have a dictionary in the bundle. */
export const AVAILABLE_LOCALES: readonly string[] = BUNDLED_LOCALES.map((l) => l.code);

/** The languages "System" may pick on its own (SYSTEM_LOCALE_MIN_PERCENT and up). */
export const SYSTEM_LOCALES: readonly string[] = systemLocaleCodes(BUNDLED_LOCALES);
