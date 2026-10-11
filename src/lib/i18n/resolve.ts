// Pure locale resolution — no Svelte/runes/IPC deps so it is trivially
// unit-testable. The persisted preference is a string: 'system' or a
// BCP-47 code ('en', 'ru', 'de', …). We never hardcode a closed set of
// languages here — `available` and `systemLocales` are discovered at runtime
// from the locale files in the bundle (dictionaries.ts), so a new community
// translation needs no code change.
import { canonicalLocaleCode } from './catalog';

export const LOCALE_STORAGE_KEY = 'locale';

/** Default UI language when nothing else resolves. */
export const FALLBACK_LOCALE = 'en';

/**
 * Map a stored preference + the OS language to a concrete locale code.
 *
 * - explicit pref that is still shipped  → that pref, however complete
 * - explicit pref that was removed       → treat as 'system'
 * - 'system'                             → the best match for the OS language
 *   among `systemLocales` (the shipped languages translated far enough, see
 *   SYSTEM_LOCALE_MIN_PERCENT), else 'en'
 *
 * `systemLocales` is required: forgetting it must not quietly let a
 * barely-translated language take over a first run.
 */
export function resolveLocale(
  pref: string,
  available: readonly string[],
  osLang: string,
  systemLocales: readonly string[],
): string {
  if (pref !== 'system' && available.includes(pref)) {
    return pref;
  }
  // system, or an explicit-but-removed language.
  const shipped = systemLocales.filter((code) => available.includes(code));
  const matched = matchSystemLocale(osLang, shipped);
  if (matched !== null) return matched;
  return available.includes(FALLBACK_LOCALE) ? FALLBACK_LOCALE : (available[0] ?? FALLBACK_LOCALE);
}

interface Tag {
  code: string;
  /** Canonical tag, lower-cased, for the exact comparison. */
  exact: string;
  language: string;
  script: string;
  region: string;
  /** Whether the tag itself names a region (`de-CH`), not just its likely one. */
  hasRegion: boolean;
}

function parseTag(code: string): Tag | null {
  if (code === '') return null;
  try {
    const own = new Intl.Locale(canonicalLocaleCode(code));
    const max = own.maximize();
    return {
      code,
      exact: own.toString().toLowerCase(),
      language: max.language,
      script: max.script ?? '',
      region: max.region ?? '',
      hasRegion: own.region !== undefined,
    };
  } catch (e) {
    if (e instanceof RangeError) return null;
    throw e;
  }
}

/** A generic `de` before a regional `de-CH`; then the first by code. */
function preferred(tags: readonly Tag[]): string | null {
  const sorted = [...tags].sort(
    (a, b) =>
      Number(a.hasRegion) - Number(b.hasRegion) || (a.code < b.code ? -1 : a.code > b.code ? 1 : 0),
  );
  return sorted[0]?.code ?? null;
}

/**
 * The candidate that best serves an OS language, or null for none. In order:
 * the exact tag (`pt-BR`); the same locale once likely subtags are filled in
 * (`zh-CN` is `zh-Hans`, `de` is `de-DE`); the same language in the same
 * script (`de-AT` → `de`, `pt-PT` → `pt-BR`). Never across scripts: Taiwan's
 * `zh-Hant` does not get `zh-Hans`, Cyrillic `sr` does not get `sr-Latn`.
 * An OS language that cannot be read matches nothing.
 */
export function matchSystemLocale(osLang: string, candidates: readonly string[]): string | null {
  const os = parseTag(osLang ?? '');
  if (os === null) return null;
  const tags = candidates.map(parseTag).filter((t): t is Tag => t !== null);
  const sameScript = tags.filter((t) => t.language === os.language && t.script === os.script);
  return (
    preferred(tags.filter((t) => t.exact === os.exact)) ??
    preferred(sameScript.filter((t) => t.region === os.region)) ??
    preferred(sameScript)
  );
}

/**
 * What the language picker shows for a stored pref. A language that is no
 * longer in the bundle resolves as System, so the picker says System rather
 * than show an empty box; the stored value is left alone, so the choice comes
 * back if the language does.
 */
export function pickerValue(pref: string, available: readonly string[]): string {
  return pref === 'system' || available.includes(pref) ? pref : 'system';
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
