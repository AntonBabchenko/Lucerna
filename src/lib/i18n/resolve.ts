// Pure locale resolution — no Svelte/runes/IPC deps so it is trivially
// unit-testable. The persisted preference is a string: 'system' or a
// BCP-47 code ('en', 'ru', 'de', …). We never hardcode a closed set of
// languages here — `available` is discovered at runtime from the locale
// files present in the bundle, so a new community translation needs no
// code change.

export const LOCALE_STORAGE_KEY = 'locale';

/** Default UI language when nothing else resolves. */
export const FALLBACK_LOCALE = 'en';

/**
 * Map a stored preference + the OS language to a concrete locale code.
 *
 * - explicit pref that is still shipped  → that pref
 * - explicit pref that was removed       → treat as 'system'
 * - 'system'                             → OS language if 'ru*', else 'en'
 *   (only if that locale is actually available; otherwise 'en')
 */
export function resolveLocale(
  pref: string,
  available: readonly string[],
  osLang: string,
): string {
  if (pref !== 'system' && available.includes(pref)) {
    return pref;
  }
  // system, or an explicit-but-removed language.
  const os = (osLang ?? '').toLowerCase();
  if (os.startsWith('ru') && available.includes('ru')) {
    return 'ru';
  }
  return available.includes(FALLBACK_LOCALE) ? FALLBACK_LOCALE : (available[0] ?? FALLBACK_LOCALE);
}

/** The OS / browser UI language as reported to the webview. */
export function getOsLang(): string {
  if (typeof navigator === 'undefined') return '';
  return navigator.language ?? '';
}

/** Read the persisted preference mirrored into localStorage (anti-FOUC). */
export function readStoredPref(): string {
  if (typeof localStorage === 'undefined') return 'system';
  try {
    return localStorage.getItem(LOCALE_STORAGE_KEY) ?? 'system';
  } catch {
    return 'system';
  }
}
