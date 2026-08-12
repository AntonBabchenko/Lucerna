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

/**
 * Filter entries to those whose localized label + keywords + section name
 * contain EVERY whitespace token (AND), ranked so a label match outranks a
 * buried keyword substring. Without ranking, "mod" would surface AI translation
 * (its keyword "model" contains "mod") above "Mod metadata cache".
 */
export function searchSettings(
  query: string,
  entries: SettingsSearchEntry[],
  t: (key: TranslationKey) => string,
): SettingsSearchEntry[] {
  const q = normalize(query);
  const tokens = q.split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return [];

  const scored: { entry: SettingsSearchEntry; score: number; order: number }[] = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    const label = normalize(t(entry.labelKey));
    const keywords = normalize(t(entry.keywordsKey));
    const section = normalize(t(`settings.sections.${entry.tab}` as TranslationKey));
    const hay = `${label} ${keywords} ${section}`;
    // AND: every token must appear somewhere in the haystack.
    if (!tokens.every((tok) => hay.includes(tok))) continue;

    // Relevance: reward matches on the label over buried keyword substrings.
    let score = 0;
    if (label.startsWith(q)) score += 100;
    else if (label.includes(q)) score += 60;
    for (const tok of tokens) {
      if (label === tok || label.startsWith(`${tok} `) || label.includes(` ${tok}`)) score += 20;
    }
    scored.push({ entry, score, order: i });
  }

  scored.sort((a, b) => b.score - a.score || a.order - b.order);
  return scored.map((s) => s.entry);
}
