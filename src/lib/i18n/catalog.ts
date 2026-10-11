// Pure facts about a locale file: its canonical BCP-47 tag, the language's
// name for itself, and how much of the English source it translates. No
// Svelte, no glob, no svelte-i18n — every input is passed in, so the bundle
// (dictionaries.ts) and the tests share one implementation.

/** "System" picks a language on its own only from this much of en.json up. */
export const SYSTEM_LOCALE_MIN_PERCENT = 90;

export interface LocaleInfo {
  /** Canonical BCP-47 tag: the svelte-i18n locale, `<html lang>`, the stored pref. */
  code: string;
  /** The language's name in itself, capitalised: «Русский», «Deutsch». */
  endonym: string;
  /** Whole percent of en.json's strings this dictionary translates, rounded down. */
  percent: number;
}

/**
 * Weblate names a new file by its POSIX code (`pt_BR.json`, `zh_Hans.json`);
 * the rest of the app needs a BCP-47 tag (`pt-BR`): `Intl` rejects `pt_BR`,
 * svelte-i18n puts the code into `<html lang>` and walks its fallback chain by
 * `-`. A name that is no language tag at all comes back unchanged — the
 * locale-file-names test keeps such a file from shipping.
 */
export function canonicalLocaleCode(raw: string): string {
  try {
    return Intl.getCanonicalLocales(raw.replace(/_/g, '-'))[0] ?? raw;
  } catch (e) {
    if (e instanceof RangeError) return raw;
    throw e;
  }
}

/**
 * The language's own name for itself, as its speakers would look for it in a
 * list: «Русский», «Français», «Português (Brasil)». Intl gives the name in
 * running-text case («русский»), so the first letter is raised by that
 * language's own rules. When Intl has no name for the code, the code is what
 * we know, and the code is what we show. That includes a language Intl has no
 * data for (Latin, Klingon): DisplayNames then quietly resolves to the HOST
 * locale and names it in the OS language («латинский» on a Russian Windows),
 * so a name is used only when it was given in the code's own language.
 */
export function localeEndonym(code: string): string {
  if (typeof Intl.DisplayNames !== 'function') return code;
  let name: string | undefined;
  try {
    const names = new Intl.DisplayNames([code], { type: 'language', fallback: 'none' });
    if (primaryLanguage(names.resolvedOptions().locale) !== primaryLanguage(code)) return code;
    // Throws too, for a tag it accepted above but cannot name (`en-u-ca-gregory`).
    name = names.of(code);
  } catch (e) {
    if (e instanceof RangeError) return code;
    throw e;
  }
  if (!name) return code;
  const [first, ...rest] = Array.from(name);
  return first.toLocaleUpperCase(code) + rest.join('');
}

/** `pt` for `pt-BR`, `zh` for `zh_Hans`: the language subtag, lower-cased. */
function primaryLanguage(tag: string): string {
  return tag.split(/[-_]/)[0].toLowerCase();
}

/** The string at `path` in a nested dictionary, or null when there is none. */
export function stringAt(dict: unknown, path: readonly string[]): string | null {
  let node: unknown = dict;
  for (const part of path) {
    if (node === null || typeof node !== 'object') return null;
    node = (node as Record<string, unknown>)[part];
  }
  return typeof node === 'string' ? node : null;
}

function stringLeafPaths(node: unknown, prefix: readonly string[] = []): string[][] {
  if (node === null || typeof node !== 'object' || Array.isArray(node)) return [];
  return Object.entries(node as Record<string, unknown>).flatMap(([k, v]) =>
    typeof v === 'string' ? [[...prefix, k]] : stringLeafPaths(v, [...prefix, k]),
  );
}

/**
 * Whole percent of the source's strings that `dict` translates, rounded down so
 * 99.9% never reads "100%". A key that is missing or holds "" is untranslated —
 * svelte-i18n shows the English string for both. A value equal to the English
 * one counts ("OK" is "OK" in many languages). Integer arithmetic: 29 of 100 is
 * 29, where `0.29 * 100` would floor to 28. An empty source translates nothing,
 * so it is 0, the answer that never auto-selects a language.
 */
export function translatedPercent(dict: unknown, source: unknown): number {
  const paths = stringLeafPaths(source);
  if (paths.length === 0) return 0;
  const translated = paths.filter((p) => (stringAt(dict, p) ?? '') !== '').length;
  return Math.floor((translated * 100) / paths.length);
}

/**
 * A locale file's canonical code from its glob path, whatever its extension:
 * `./locales/pt_BR.json` and `./locales/pt_BR.md` are both `pt-BR`. '' when the
 * path names no file. The UI dictionaries and the changelog translations key
 * by this one function, so a code means the same language in both.
 */
export function localeCodeFromPath(path: string): string {
  return canonicalLocaleCode(path.match(/\/([^/]+)\.[^./]+$/)?.[1] ?? '');
}

export interface BundledLocale extends LocaleInfo {
  dict: unknown;
}

/**
 * The bundle from an eager glob of dictionaries: one entry per file, keyed by
 * its canonical code, sorted by code, each measured against the dictionary of
 * `sourceCode` (none: every language measures 0, so System picks none).
 */
export function bundleLocales(
  modules: Readonly<Record<string, { default: unknown }>>,
  sourceCode: string,
): BundledLocale[] {
  const found = Object.entries(modules)
    .map(([path, mod]) => ({ code: localeCodeFromPath(path), dict: mod.default }))
    .filter((l) => l.code !== '')
    .sort((a, b) => (a.code < b.code ? -1 : a.code > b.code ? 1 : 0));
  const source = found.find((l) => l.code === sourceCode)?.dict;
  return found.map(({ code, dict }) => ({
    code,
    dict,
    endonym: localeEndonym(code),
    percent: translatedPercent(dict, source),
  }));
}

/**
 * The languages "System" may pick on its own: translated far enough that the
 * UI does not come out as a mix with English. Any of the others can still be
 * chosen by hand.
 */
export function systemLocaleCodes(bundle: readonly LocaleInfo[]): string[] {
  return bundle.filter((l) => l.percent >= SYSTEM_LOCALE_MIN_PERCENT).map((l) => l.code);
}
