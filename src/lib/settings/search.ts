// Pure settings-search matcher. No component state, no IPC — trivially
// unit-testable. `t` is passed in (the caller supplies the reactive $t) so the
// haystack is always the current locale's strings.
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { SettingsSearchEntry } from './search-index';

/**
 * Lowercase, trim, and strip combining marks so "Тёмная"/"темная" and
 * "café"/"cafe" match — but keep й: NFD would split it into и + a breve and
 * the stripper would fold it into и ("дизайн" ≠ "дизаин").
 */
export function normalize(s: string): string {
  return (
    s
      // Private-use code points stand in for й/Й: no combining mark, no case,
      // never in real text — so NFD and toLowerCase pass them through untouched.
      .replace(/й/g, '\uE000')
      .replace(/Й/g, '\uE001')
      .normalize('NFD')
      .replace(/[̀-ͯ]/g, '')
      .toLowerCase()
      .replace(/[\uE000\uE001]/g, 'й')
      .trim()
  );
}

/** The words of a string: runs of letters and digits; an apostrophe stays inside a word ("what's"). */
export function words(s: string): string[] {
  return normalize(s)
    .split(/[^\p{L}\p{N}'’]+/u)
    .map((w) => w.replace(/^['’]+|['’]+$/g, ''))
    .filter(Boolean);
}

/**
 * A query token matches a haystack word by shared prefix. A short token
 * (≤ 3 letters: "ai", "gpu", "ии") must be an exact word start — never a
 * substring, so "ai" does not find "rainbow". A longer token needs a common
 * prefix of at least four letters and at least all but two of the shorter
 * word: "тёмный"/"тёмная", "видеокарту"/"видеокарта", "icons"/"icon" match;
 * "logs"/"log", "data"/"date", and any 1–2-letter word do not.
 */
export function matchesWord(token: string, word: string): boolean {
  if (token.length <= 3) return word.startsWith(token);
  const shorter = Math.min(token.length, word.length);
  let p = 0;
  while (p < shorter && token[p] === word[p]) p++;
  return p >= 4 && p >= shorter - 2;
}

type WordClass = 'label' | 'visible' | 'keyword' | 'section';
/** An exact word beats a stem in every class; the label beats the page's words beat the keywords. */
const EXACT: Record<WordClass, number> = { label: 35, visible: 30, keyword: 25, section: 5 };
const STEM: Record<WordClass, number> = { label: 20, visible: 15, keyword: 10, section: 0 };

function bestHit(token: string, list: string[]): 'exact' | 'stem' | null {
  let stem = false;
  for (const w of list) {
    if (w === token) return 'exact';
    if (!stem && matchesWord(token, w)) stem = true;
  }
  return stem ? 'stem' : null;
}

/**
 * Filter entries to those where EVERY query token matches some word of the
 * label, the control's visible strings, the keywords or the section name
 * (AND), ranked: the whole query at the label's start, then per token the
 * best class hit. Ties keep registry order.
 */
export function searchSettings(
  query: string,
  entries: SettingsSearchEntry[],
  t: (key: TranslationKey) => string,
): SettingsSearchEntry[] {
  const q = normalize(query);
  const tokens = words(q);
  if (tokens.length === 0) return [];

  const scored: { entry: SettingsSearchEntry; score: number; order: number }[] = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    const label = normalize(t(entry.labelKey));
    const labelWords = words(label);
    const classes: [WordClass, string[]][] = [
      ['label', labelWords],
      ['visible', entry.visibleKeys.flatMap((k) => words(t(k)))],
      ['keyword', words(t(entry.keywordsKey))],
      ['section', words(t(`settings.sections.${entry.tab}` as TranslationKey))],
    ];
    let score = 0;
    // The whole query at the label's start, or as whole words inside it — a
    // substring bonus would hand "local" to "Data location" again.
    if (label.startsWith(q)) score += 100;
    else if (` ${labelWords.join(' ')} `.includes(` ${q} `)) score += 60;
    let every = true;
    for (const tok of tokens) {
      let best = -1;
      for (const [cls, list] of classes) {
        const hit = bestHit(tok, list);
        if (hit) best = Math.max(best, hit === 'exact' ? EXACT[cls] : STEM[cls]);
      }
      if (best < 0) {
        every = false;
        break;
      }
      score += best;
    }
    if (!every) continue;
    scored.push({ entry, score, order: i });
  }

  scored.sort((a, b) => b.score - a.score || a.order - b.order);
  return scored.map((s) => s.entry);
}
