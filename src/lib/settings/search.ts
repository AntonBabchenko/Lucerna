// Pure settings-search matcher. No component state, no IPC — trivially
// unit-testable. `t` is passed in (the caller supplies the reactive $t) so the
// haystack is always the current locale's strings.
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { SettingsSearchEntry } from './search-index';

/** Lowercase, trim, and strip combining marks so "Тёмная"/"темная" and
 *  "café"/"cafe" match. NFD splits ё into е + combining diaeresis; the range
 *  ̀-ͯ drops the mark. */
export function normalize(s: string): string {
  return s
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .trim();
}

export function searchSettings(
  query: string,
  entries: SettingsSearchEntry[],
  t: (key: TranslationKey) => string,
): SettingsSearchEntry[] {
  const tokens = normalize(query).split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return [];
  return entries.filter((e) => {
    const sectionKey = `settings.sections.${e.tab}` as TranslationKey;
    const hay = normalize(`${t(e.labelKey)} ${t(e.keywordsKey)} ${t(sectionKey)}`);
    return tokens.every((tok) => hay.includes(tok));
  });
}
