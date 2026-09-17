// Lay a translated changelog over the English one. English (CHANGELOG.md) is
// the source of truth for *structure* — which versions exist, their dates and
// links, section kinds, how many bullets — so the post-update prompt, the
// version list and the labels never depend on a translation being complete.
// A translation only contributes text, matched by version label and then by
// section and bullet position (a Keep-a-Changelog mirror, see locales.ts).
//
// Fallback discipline: whenever the translation cannot be trusted for a
// version — absent, or its section/bullet counts differ so positions would
// pair the wrong texts — the whole version is shown in English and marked so
// the panel can say it. A bullet whose text equals the English one is shown
// as English too (that is what an untranslated paragraph looks like in a file
// a translation tool re-emits from the source).
import type { Changelog, ChangelogVersion, DisplayVersion } from './types';

function bulletCount(v: ChangelogVersion): number {
  return v.sections.reduce((n, s) => n + s.items.length, 0);
}

function fromSource(v: ChangelogVersion, localized: boolean): DisplayVersion {
  return {
    version: v.version,
    date: v.date,
    url: v.url,
    sections: v.sections.map((s) => ({
      kind: s.kind,
      heading: s.heading,
      items: s.items.map((text) => ({ text, localized })),
    })),
    coverage: localized || bulletCount(v) === 0 ? 'full' : 'none',
  };
}

/** The source language view: nothing to overlay, nothing to flag. */
export function asSourceLanguage(source: Changelog): DisplayVersion[] {
  return source.map((v) => fromSource(v, true));
}

function aligned(src: ChangelogVersion, tr: ChangelogVersion): boolean {
  return (
    src.sections.length === tr.sections.length &&
    src.sections.every((s, i) => s.items.length === tr.sections[i].items.length)
  );
}

/**
 * Overlay `translation` on `source`. `null` means the locale has no usable
 * translation at all; every version then comes back as source text with
 * coverage 'none'.
 */
export function localizeChangelog(
  source: Changelog,
  translation: Changelog | null,
): DisplayVersion[] {
  return source.map((v) => {
    const tr = translation?.find((x) => x.version === v.version);
    if (!tr || !aligned(v, tr)) return fromSource(v, false);

    let localizedCount = 0;
    const sections = v.sections.map((s, i) => {
      const ts = tr.sections[i];
      return {
        kind: s.kind,
        heading: ts.heading,
        items: s.items.map((text, j) => {
          const candidate = ts.items[j];
          const localized = candidate !== text;
          if (localized) localizedCount++;
          return { text: localized ? candidate : text, localized };
        }),
      };
    });
    const total = bulletCount(v);
    const coverage = localizedCount === total ? 'full' : localizedCount === 0 ? 'none' : 'partial';
    return { version: v.version, date: v.date, url: v.url, sections, coverage };
  });
}
