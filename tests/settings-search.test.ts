import { describe, expect, it } from 'vitest';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import en from '$lib/i18n/locales/en.json';
import ru from '$lib/i18n/locales/ru.json';
import { matchesWord, normalize, searchSettings, words } from '$lib/settings/search';
import type { SettingsAnchor, SettingsSearchEntry } from '$lib/settings/search-index';
import { SETTINGS_ENTRIES } from '$lib/settings/search-index';

const ENTRIES: SettingsSearchEntry[] = [
  {
    anchor: 'game.gpu',
    tab: 'game',
    labelKey: 'L.gpu' as never,
    keywordsKey: 'K.gpu' as never,
    visibleKeys: [],
  },
  {
    anchor: 'appearance.theme',
    tab: 'appearance',
    labelKey: 'L.theme' as never,
    keywordsKey: 'K.theme' as never,
    visibleKeys: [],
  },
];

// Ranking fixtures: an exact keyword against a label stem, and visible words.
const RANK: SettingsSearchEntry[] = [
  {
    anchor: 'storage.dataLocation',
    tab: 'storage',
    labelKey: 'L.data' as never,
    keywordsKey: 'K.data' as never,
    visibleKeys: ['V.data' as never],
  },
  {
    anchor: 'integrations.aiTranslation',
    tab: 'integrations',
    labelKey: 'L.ai' as never,
    keywordsKey: 'K.ai' as never,
    visibleKeys: ['V.ai' as never],
  },
  {
    anchor: 'updates.startupCheck',
    tab: 'updates',
    labelKey: 'L.startup' as never,
    keywordsKey: 'K.startup' as never,
    visibleKeys: ['V.startup' as never],
  },
  {
    anchor: 'storage.logRetention',
    tab: 'storage',
    labelKey: 'L.logs' as never,
    keywordsKey: 'K.logs' as never,
    visibleKeys: ['V.logs' as never],
  },
];
const RANK_STRINGS: Record<string, string> = {
  'L.data': 'Data location',
  'K.data': 'data folder disk',
  'V.data': 'Data location',
  'L.ai': 'AI translation',
  'K.ai': 'ai local model claude',
  'V.ai': 'AI translation',
  'L.startup': 'Check for updates on startup',
  'K.startup': 'update startup auto',
  'V.startup': 'Check for updates on startup',
  'L.logs': 'Log retention',
  'K.logs': 'log size',
  'V.logs': 'Automatically delete old logs',
  'settings.sections.storage': 'Storage',
  'settings.sections.integrations': 'Integrations',
  'settings.sections.updates': 'Updates',
};
const rankT = (k: string): string => RANK_STRINGS[k] ?? k;
const rank = (q: string) => searchSettings(q, RANK, rankT as never).map((e) => e.anchor);

const STRINGS: Record<string, string> = {
  'L.gpu': 'Preferred GPU',
  'K.gpu': 'gpu graphics видеокарта',
  'L.theme': 'Theme',
  'K.theme': 'theme тёмная',
  'settings.sections.game': 'Game',
  'settings.sections.appearance': 'Appearance',
};
const t = (k: string): string => STRINGS[k] ?? k;

describe('normalize', () => {
  it('lowercases, trims, and strips diacritics (incl. Cyrillic ё → е)', () => {
    expect(normalize('  Тёмная ')).toBe('темная');
    expect(normalize('GPU')).toBe('gpu');
  });

  it('keeps й while still folding ё', () => {
    expect(normalize('чай')).toBe('чай');
    expect(normalize('Тёмная')).toBe('темная');
  });
});

describe('words', () => {
  it('splits on anything but letters and digits, keeping an apostrophe inside a word', () => {
    expect(words("What's new: ИИ-перевод (beta)")).toEqual([
      "what's",
      'new',
      'ии',
      'перевод',
      'beta',
    ]);
  });
});

describe('matchesWord', () => {
  it('a short token matches only a word start', () => {
    expect(matchesWord('ai', 'rainbow')).toBe(false);
    expect(matchesWord('ai', 'ai')).toBe(true);
    expect(matchesWord('gpu', 'gpus')).toBe(true);
  });

  it('a long token matches a word by a shared prefix of at least four letters and all but two', () => {
    expect(matchesWord('темный', 'темная')).toBe(true);
    expect(matchesWord('видеокарту', 'видеокарта')).toBe(true);
    expect(matchesWord('icons', 'icon')).toBe(true);
    expect(matchesWord('папку', 'папка')).toBe(true);
    expect(matchesWord('logs', 'log')).toBe(false);
    expect(matchesWord('data', 'date')).toBe(false);
    expect(matchesWord('tooltip', 'to')).toBe(false);
  });
});

describe('ranking', () => {
  it('an exact keyword hit outranks a label stem hit', () => {
    expect(rank('local')[0]).toBe('integrations.aiTranslation');
    expect(rank('auto')[0]).toBe('updates.startupCheck');
  });

  it("visible words are matched, so the page's own words find the control", () => {
    expect(rank('delete')).toEqual(['storage.logRetention']);
    expect(rank('logs')).toEqual(['storage.logRetention']);
  });
});

describe('searchSettings', () => {
  it('returns [] for an empty or whitespace query', () => {
    expect(searchSettings('', ENTRIES, t as never)).toEqual([]);
    expect(searchSettings('   ', ENTRIES, t as never)).toEqual([]);
  });

  it('matches an English keyword case-insensitively', () => {
    expect(searchSettings('GRAPHICS', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'game.gpu',
    ]);
  });

  it('matches a Russian keyword ignoring diacritics (темная → тёмная)', () => {
    expect(searchSettings('темная', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'appearance.theme',
    ]);
  });

  it('requires every whitespace token to match (AND semantics)', () => {
    expect(searchSettings('preferred gpu', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'game.gpu',
    ]);
    expect(searchSettings('preferred theme', ENTRIES, t as never)).toEqual([]);
  });

  it('also matches on the section name', () => {
    expect(searchSettings('appearance', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'appearance.theme',
    ]);
  });

  it('returns [] when nothing matches', () => {
    expect(searchSettings('zzzz', ENTRIES, t as never)).toEqual([]);
  });
});

function localeT(dict: unknown): (key: TranslationKey) => string {
  return (key) =>
    String(
      key.split('.').reduce<unknown>((o, part) => {
        if (o && typeof o === 'object') return (o as Record<string, unknown>)[part];
        return undefined;
      }, dict) ?? '',
    );
}

describe('searchSettings over the real registry + locales', () => {
  type T = (key: TranslationKey) => string;
  const enT = localeT(en);
  const ruT = localeT(ru);
  const top = (q: string, t: T) => searchSettings(q, SETTINGS_ENTRIES, t).map((e) => e.anchor);

  // Every probe comes from a finding of the 2026-09-21 Settings audit or from
  // a pin that predates it. `first` = the top result; `has` = present;
  // `not` = absent; `only` = the whole result list.
  const CASES: {
    q: string;
    t: T;
    first?: SettingsAnchor;
    has?: SettingsAnchor[];
    not?: SettingsAnchor[];
    only?: SettingsAnchor[];
  }[] = [
    { q: 'gpu', t: enT, first: 'game.gpu' },
    { q: 'language', t: enT, first: 'appearance.language' },
    { q: 'data', t: enT, first: 'storage.dataLocation' },
    { q: 'тема', t: ruT, first: 'appearance.theme' },
    { q: 'язык', t: ruT, first: 'appearance.language' },
    { q: 'видеокарта', t: ruT, first: 'game.gpu' },
    { q: 'auto', t: enT, first: 'updates.startupCheck' },
    { q: 'диск', t: ruT, first: 'storage.dataLocation' },
    { q: 'dark mode', t: enT, first: 'appearance.theme' },
    { q: 'logs', t: enT, first: 'storage.logRetention' },
    { q: 'graphics card', t: enT, first: 'game.gpu' },
    { q: 'nvidia', t: enT, first: 'game.gpu' },
    { q: 'tutorial', t: enT, has: ['help.tipsLevel', 'help.replayTours'] },
    { q: 'directory', t: enT, first: 'storage.dataLocation' },
    { q: 'info', t: enT, first: 'storage.modMetadataCache' },
    { q: 'release notes', t: enT, first: 'updates.changelog' },
    { q: 'hide launcher', t: enT, first: 'game.tray' },
    { q: 'minimise', t: enT, first: 'game.tray' },
    { q: 'свернуть', t: ruT, first: 'game.tray' },
    { q: 'когда запускается игра', t: ruT, first: 'game.tray' },
    { q: 'claude', t: enT, first: 'integrations.aiTranslation' },
    { q: 'ollama', t: enT, first: 'integrations.aiTranslation' },
    { q: 'local', t: enT, first: 'integrations.aiTranslation' },
    { q: 'русский', t: enT, first: 'appearance.language' },
    { q: 'privacy', t: enT, has: ['game.serverPing', 'integrations.aiTranslation'] },
    { q: 'ai', t: enT, first: 'integrations.aiTranslation', not: ['appearance.rainbowIcons'] },
    // Re-pinned in batch 12a: About now shows the version itself (and copies it),
    // the better answer to "which version do I have"; the changelog stays second.
    {
      q: 'version',
      t: enT,
      first: 'about.versionInfo',
      has: ['updates.changelog'],
      not: ['about.repo'],
    },
    { q: 'bug', t: enT, first: 'help.support' },
    { q: 'ошибка', t: ruT, first: 'help.support' },
    { q: 'licence', t: enT, first: 'about.license' },
    { q: 'лицензия', t: ruT, first: 'about.license' },
    { q: 'logs', t: enT, has: ['help.logFolder'] },
    { q: 'приватность', t: ruT, first: 'about.privacyPolicy' },
    { q: 'show hidden', t: enT, first: 'appearance.sidebarButtons' },
    { q: 'startup', t: enT, first: 'updates.startupCheck' },
    { q: 'тёмный режим', t: ruT, first: 'appearance.theme' },
    { q: 'видеокарту', t: ruT, first: 'game.gpu' },
    // Batch 12c: the Storage page's «Открыть папку данных» ties the Help log folder on the
    // exact visible word and wins on registry order; the data-location block stays in the list.
    {
      q: 'папку',
      t: ruT,
      first: 'storage.openDataFolder',
      has: ['storage.dataLocation', 'help.logFolder'],
    },
    { q: 'open folder', t: enT, first: 'storage.openDataFolder', has: ['help.logFolder'] },
    { q: 'перенести данные', t: ruT, first: 'storage.dataLocation' },
    { q: 'кеш', t: ruT, first: 'storage.cache', has: ['storage.modMetadataCache'] },
    {
      q: 'иконки',
      t: ruT,
      has: ['appearance.rainbowIcons', 'appearance.iconZoom', 'storage.modMetadataCache'],
    },
    { q: 'знакомство', t: ruT, first: 'help.replayTours' },
    { q: 'русификация', t: ruT, first: 'integrations.aiTranslation' },
    { q: 'история', t: ruT, first: 'updates.changelog' },
    { q: 'подсказки', t: ruT, has: ['help.tipsLevel', 'help.replayTours'] },
    { q: 'ии', t: ruT, only: ['integrations.aiTranslation'] },
    { q: 'language', t: ruT, first: 'appearance.language' },
    { q: 'english', t: ruT, first: 'appearance.language' },
    { q: 'запуск', t: ruT, first: 'updates.startupCheck' },
    { q: 'вернуть', t: ruT, first: 'appearance.sidebarButtons' },
  ];

  for (const c of CASES) {
    it(`"${c.q}" → ${c.first ?? c.only?.join('+') ?? c.has?.join(' + ')}`, () => {
      const r = top(c.q, c.t);
      if (c.first) expect(r[0]).toBe(c.first);
      if (c.only) expect(r).toEqual(c.only);
      for (const a of c.has ?? []) expect(r).toContain(a);
      for (const a of c.not ?? []) expect(r).not.toContain(a);
    });
  }

  // A tab's own name is the one query whose only signal is the section class:
  // it must still beat a stem of an unrelated keyword ("integrations" vs
  // gpu's "intel", «обновления» vs the mod cache's «обновление»).
  it("typing a tab's own name puts that tab's settings first, in both locales", () => {
    const tabs = [...new Set(SETTINGS_ENTRIES.map((e) => e.tab))];
    expect(tabs.length).toBe(7);
    for (const t of [enT, ruT]) {
      for (const tab of tabs) {
        const q = t(`settings.sections.${tab}` as TranslationKey);
        const r = searchSettings(q, SETTINGS_ENTRIES, t);
        expect(r.length, q).toBeGreaterThan(0);
        expect(r[0].tab, q).toBe(tab);
      }
    }
  });

  it('"mod" / «мод» put the two mod caches first and never AI translation', () => {
    for (const [q, t] of [
      ['mod', enT],
      ['мод', ruT],
    ] as const) {
      const r = top(q, t);
      expect(new Set(r.slice(0, 2))).toEqual(
        new Set(['storage.cache', 'storage.modMetadataCache']),
      );
      expect(r.slice(0, 2)).not.toContain('integrations.aiTranslation');
    }
  });
});
