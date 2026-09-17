import { describe, expect, it } from 'vitest';
import ru from '$lib/changelog/locales/ru.md?raw';
import { parseChangelog } from '$lib/changelog/parse';
import en from '../CHANGELOG.md?raw';

// Russian ships alongside English, so — exactly like tests/i18n-parity.test.ts
// for ru.json — the Russian changelog must mirror CHANGELOG.md completely:
// same versions in the same order, same dates and links, the same number of
// sections and bullets, and every bullet actually translated. Section headings
// may be translated freely (the kind is taken from English at runtime).
describe('changelog locale parity (en vs ru)', () => {
  const EN = parseChangelog(en);
  const RU = parseChangelog(ru);

  it('parses both files into at least one version', () => {
    expect(EN.length).toBeGreaterThan(0);
    expect(RU.length).toBeGreaterThan(0);
  });

  it('has the same versions in the same order, with the same dates and links', () => {
    const shape = (v: (typeof EN)[number]) => ({ version: v.version, date: v.date, url: v.url });
    expect(RU.map(shape)).toEqual(EN.map(shape));
  });

  it('has the same number of sections per version and bullets per section', () => {
    const counts = (v: (typeof EN)[number]) => ({
      version: v.version,
      sections: v.sections.map((s) => s.items.length),
    });
    expect(RU.map(counts)).toEqual(EN.map(counts));
  });

  // Walk every bullet of EN next to its positional twin in RU (or '' when RU
  // is shorter — the count test above already reports that).
  function* bullets(): Generator<{ where: string; en: string; ru: string }> {
    for (const [vi, v] of EN.entries()) {
      for (const [si, s] of v.sections.entries()) {
        for (const [ii, item] of s.items.entries()) {
          yield {
            where: `${v.version} / ${s.heading} / #${ii + 1}`,
            en: item,
            ru: RU[vi]?.sections[si]?.items[ii] ?? '',
          };
        }
      }
    }
  }

  it('translates every bullet (no bullet left equal to its English source)', () => {
    const untranslated: string[] = [];
    for (const b of bullets()) if (b.ru === b.en) untranslated.push(b.where);
    expect(untranslated).toEqual([]);
  });

  // Inline code spans are identifiers (crate names, flags, paths, keys) and
  // must survive translation verbatim, as the same multiset — the changelog's
  // analogue of the ICU-placeholder check in i18n-parity.
  it('keeps every inline code span of a bullet', () => {
    const spans = (s: string) => (s.match(/`[^`]+`/g) ?? []).sort();
    const mismatches: string[] = [];
    for (const b of bullets()) {
      const a = spans(b.en);
      const c = spans(b.ru);
      if (JSON.stringify(a) !== JSON.stringify(c)) {
        mismatches.push(`${b.where}: en ${a} vs ru ${c}`);
      }
    }
    expect(mismatches).toEqual([]);
  });

  it('keeps the bold lead-in where the English bullet has one', () => {
    const mismatches: string[] = [];
    for (const b of bullets()) {
      if (b.en.startsWith('**') !== b.ru.startsWith('**')) mismatches.push(b.where);
    }
    expect(mismatches).toEqual([]);
  });
});
