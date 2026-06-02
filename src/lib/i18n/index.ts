// Single import surface for i18n. At module load it (1) discovers every
// locale JSON in ./locales, registers each synchronously (no async
// loader → no flash-of-keys), and (2) calls svelte-i18n init() so the
// `$t` / `$_` stores are usable on first paint. The boot sequence later
// reconciles the actual persisted preference via initLocale().
import { addMessages, init, locale } from 'svelte-i18n';
import { FALLBACK_LOCALE, getOsLang, readStoredPref, resolveLocale } from './resolve';

// LocaleDictionary is declared but not publicly exported by svelte-i18n.
// Cast the glob result through `unknown` so addMessages() receives the right shape
// without importing from an internal path.
const modules = import.meta.glob('./locales/*.json', { eager: true }) as Record<
  string,
  { default: unknown }
>;

/** Sorted list of locale codes that have a dictionary in the bundle. */
export const AVAILABLE_LOCALES: string[] = Object.entries(modules)
  .map(([path, mod]) => {
    const code = path.match(/\/([^/]+)\.json$/)?.[1] ?? '';
    addMessages(code, mod.default as Parameters<typeof addMessages>[1]);
    return code;
  })
  .filter(Boolean)
  .sort();

// Initialise immediately with the best guess from localStorage + OS, so
// components that subscribe to $t during first render get real strings.
init({
  fallbackLocale: FALLBACK_LOCALE,
  initialLocale: resolveLocale(readStoredPref(), AVAILABLE_LOCALES, getOsLang()),
});

export { t } from './typed';
export { locale };
