import { describe, expect, it } from 'vitest';
import { asSourceLanguage, localizeChangelog } from '$lib/changelog/localize';
import type { Changelog } from '$lib/changelog/types';

// English is the structural source of truth; a translation is overlaid by
// version label, then by section and bullet position.
const EN: Changelog = [
  {
    version: '0.2.0',
    date: '2026-02-02',
    url: 'https://example.test/v0.2.0',
    sections: [
      { kind: 'added', heading: 'Added', items: ['**New thing.** It does `x`.', 'Second entry.'] },
      { kind: 'other', heading: 'Notes', items: ['A note.'] },
    ],
  },
  {
    version: '0.1.0',
    date: '2026-01-01',
    url: null,
    sections: [{ kind: 'added', heading: 'Added', items: ['First release.'] }],
  },
];

const RU_0_2_0: Changelog[number] = {
  version: '0.2.0',
  date: '2026-02-02',
  url: 'https://example.test/v0.2.0',
  sections: [
    // Translated headings are not recognized kinds — the kind comes from EN.
    {
      kind: 'other',
      heading: 'Добавлено',
      items: ['**Новая штука.** Делает `x`.', 'Вторая запись.'],
    },
    { kind: 'other', heading: 'Заметки', items: ['Заметка.'] },
  ],
};

describe('localizeChangelog', () => {
  it('substitutes bullet text by position; structure, date, url and kind come from the source', () => {
    const [v2, v1] = localizeChangelog(EN, [RU_0_2_0]);
    expect(v2.version).toBe('0.2.0');
    expect(v2.date).toBe('2026-02-02');
    expect(v2.url).toBe('https://example.test/v0.2.0');
    expect(v2.coverage).toBe('full');
    expect(v2.sections[0].kind).toBe('added');
    expect(v2.sections[0].items.map((i) => i.text)).toEqual([
      '**Новая штука.** Делает `x`.',
      'Вторая запись.',
    ]);
    expect(v2.sections[0].items.every((i) => i.localized)).toBe(true);
    // 0.1.0 is not in the translation at all.
    expect(v1.coverage).toBe('none');
    expect(v1.sections[0].items[0]).toEqual({ text: 'First release.', localized: false });
  });

  it('renders the translated heading of an unrecognized section, but the source kind', () => {
    const [v2] = localizeChangelog(EN, [RU_0_2_0]);
    expect(v2.sections[1].kind).toBe('other');
    expect(v2.sections[1].heading).toBe('Заметки');
  });

  it('falls back to the source for the whole version when the section count differs', () => {
    const misaligned: Changelog = [{ ...RU_0_2_0, sections: [RU_0_2_0.sections[0]] }];
    const [v2] = localizeChangelog(EN, misaligned);
    expect(v2.coverage).toBe('none');
    expect(v2.sections.map((s) => s.heading)).toEqual(['Added', 'Notes']);
    expect(v2.sections[0].items[0].text).toBe('**New thing.** It does `x`.');
  });

  it('falls back to the source for the whole version when a bullet count differs', () => {
    const misaligned: Changelog = [
      {
        ...RU_0_2_0,
        sections: [{ ...RU_0_2_0.sections[0], items: ['только один пункт'] }, RU_0_2_0.sections[1]],
      },
    ];
    const [v2] = localizeChangelog(EN, misaligned);
    expect(v2.coverage).toBe('none');
    expect(v2.sections[0].items.map((i) => i.text)).toEqual([
      '**New thing.** It does `x`.',
      'Second entry.',
    ]);
  });

  it('marks a bullet whose text equals the source as not localized, and the version as partial', () => {
    const partial: Changelog = [
      {
        ...RU_0_2_0,
        sections: [
          { ...RU_0_2_0.sections[0], items: ['**Новая штука.** Делает `x`.', 'Second entry.'] },
          RU_0_2_0.sections[1],
        ],
      },
    ];
    const [v2] = localizeChangelog(EN, partial);
    expect(v2.coverage).toBe('partial');
    expect(v2.sections[0].items[0].localized).toBe(true);
    expect(v2.sections[0].items[1]).toEqual({ text: 'Second entry.', localized: false });
  });

  it('with no translation at all, every version is the source text and marked none', () => {
    const out = localizeChangelog(EN, null);
    expect(out.map((v) => v.coverage)).toEqual(['none', 'none']);
    expect(out[0].sections[0].items[0]).toEqual({
      text: '**New thing.** It does `x`.',
      localized: false,
    });
  });

  it('a version with no sections has full coverage (nothing to translate)', () => {
    const empty: Changelog = [{ version: 'Unreleased', date: null, url: null, sections: [] }];
    expect(localizeChangelog(empty, null)[0].coverage).toBe('full');
  });
});

describe('asSourceLanguage', () => {
  it('shows every version as fully covered in the source language', () => {
    const out = asSourceLanguage(EN);
    expect(out.map((v) => v.coverage)).toEqual(['full', 'full']);
    expect(out[0].sections[0].items[0]).toEqual({
      text: '**New thing.** It does `x`.',
      localized: true,
    });
    expect(out[0].sections[1].heading).toBe('Notes');
  });
});
