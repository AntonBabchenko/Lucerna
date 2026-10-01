import { describe, expect, it } from 'vitest';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

type Json = Record<string, unknown>;

function flatten(obj: Json, prefix = ''): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      Object.assign(out, flatten(v as Json, path));
    } else {
      out[path] = v as string;
    }
  }
  return out;
}

describe('i18n locale parity (en vs ru)', () => {
  const flatEn = flatten(en as Json);
  const flatRu = flatten(ru as Json);

  it('ru has exactly the same keys as en', () => {
    const enKeys = Object.keys(flatEn).sort();
    const ruKeys = Object.keys(flatRu).sort();
    const missingInRu = enKeys.filter((k) => !(k in flatRu));
    const extraInRu = ruKeys.filter((k) => !(k in flatEn));
    expect({ missingInRu, extraInRu }).toEqual({ missingInRu: [], extraInRu: [] });
  });

  it('no value is empty in either locale', () => {
    const emptyEn = Object.entries(flatEn)
      .filter(([, v]) => !v || !v.trim())
      .map(([k]) => k);
    const emptyRu = Object.entries(flatRu)
      .filter(([, v]) => !v || !v.trim())
      .map(([k]) => k);
    expect({ emptyEn, emptyRu }).toEqual({ emptyEn: [], emptyRu: [] });
  });

  // ICU argument placeholders (e.g. {count}, {url}) must be identical between
  // locales — a translation that drops or renames a placeholder silently
  // breaks interpolation at runtime. We extract only ARGUMENT names ({name,
  // or {name}) and deliberately ignore ICU sub-message text like
  // `one {# mod}`, whose leading word differs by language.
  function argPlaceholders(value: string): string[] {
    return [...value.matchAll(/\{\s*(\w+)\s*[,}]/g)].map((m) => m[1]).sort();
  }

  // Deliberately NOT asserted here: that a key which is a plural message in one
  // locale is one in both. Russian frequently sidesteps plural agreement with a
  // colon form ("Удалено модов: {count}"), which is correct for every count and
  // is a translator's choice, not a defect — a symmetry rule would ban it. The
  // hazard that shape mismatch creates (a caller reads the plain `en` value and
  // pre-formats the number, breaking the other locale's plural selector) is
  // covered directly by tests/i18n-plural-args.test.ts, which derives its key
  // set from BOTH locales.
  it('ICU argument placeholders match between en and ru', () => {
    const mismatches = Object.keys(flatEn)
      .filter((k) => k in flatRu)
      .map((k) => ({ k, en: argPlaceholders(flatEn[k]), ru: argPlaceholders(flatRu[k]) }))
      .filter(({ en, ru }) => en.join(',') !== ru.join(','))
      .map(({ k, en, ru }) => `${k} (en:[${en}] ru:[${ru}])`);
    expect(mismatches).toEqual([]);
  });

  // Keys whose last consumer is gone. No unused-key tool exists, so the
  // absence is pinned here: a dead key is a string a translator maintains
  // for nothing, and a key that comes back is a key someone will read.
  it('carries none of the keys retired by the Settings a11y pass', () => {
    const retired = [
      // Batch 12d: the tray checkbox became the 'When a game starts' choice.
      'settings.general.playing.trayLabel',
      'settings.closeBackdropLabel', // no reference anywhere in src/ (dead since the shared Modal)
      'settings.about.openRepoLabel', // the GitHub button is named by its visible text
      'settings.changelog.openReleaseLabel', // the version button is named by the version
    ];
    expect(retired.filter((k) => k in flatEn || k in flatRu)).toEqual([]);
  });

  // Batch 11a fixed one vocabulary for two concepts: the guided walkthrough is
  // "Tour" / «Тур», and the detail level is "Explanations" / «Объяснения».
  // The old names are kept ON PURPOSE as search keywords — someone who learned
  // "onboarding" must still find the row — so the sweep covers every OTHER
  // string. Without this, a rename that misses the tour's own Skip button
  // ships a vocabulary that is one word per thing everywhere except on the
  // thing itself.
  it('uses one name for the tour in every string a user can read', () => {
    const LEGACY_KEYWORD_PREFIX = 'settings.search.keywords.';
    const BANNED = [
      { re: /onboarding/i, what: 'en "onboarding"' },
      { re: /welcome tour/i, what: 'en "welcome tour"' },
      { re: /guided tips/i, what: 'en "guided tips"' },
      { re: /обучени/i, what: 'ru «обучение»' },
      { re: /знакомств/i, what: 'ru «знакомство»' },
      { re: /вводный тур/i, what: 'ru «вводный тур»' },
    ];
    const offenders: string[] = [];
    for (const [flat, locale] of [
      [flatEn, 'en'],
      [flatRu, 'ru'],
    ] as const) {
      for (const [key, value] of Object.entries(flat)) {
        if (key.startsWith(LEGACY_KEYWORD_PREFIX)) continue;
        if (typeof value !== 'string') continue;
        for (const { re, what } of BANNED) {
          if (re.test(value)) offenders.push(`${locale}:${key} still says ${what}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });

  // D14 (installed-mods UX program): Russian says one word per thing — «профиль», «загрузчик»,
  // «сборка» — and spells «ресурспак» one way. Search keywords are exempt on purpose, like the
  // tour's legacy names above: someone who types the slang must still find the row.
  it('uses one Russian word per thing: профиль, загрузчик, сборка, ресурспак', () => {
    const BANNED = [/инстанс/i, /экземпляр/i, /лоадер/i, /модпак/i, /ресурс-пак/i];
    const offenders = Object.entries(flatRu)
      .filter(([key]) => !key.startsWith('settings.search.keywords.'))
      .filter(([, value]) => typeof value === 'string' && BANNED.some((re) => re.test(value)))
      .map(([key]) => key);
    expect(offenders).toEqual([]);
  });

  // «Сборка» is the modpack (D14), so a BUILD — of a mod, a loader, OptiFine, the launcher — is
  // never «сборка»: «Нет сборки Forge» read as "no Forge modpack". Found by what the English says:
  // a build that is not a pack. Search keywords are exempt, as above.
  it('never says «сборка» for a build', () => {
    const offenders = Object.entries(flatRu)
      .filter(([key]) => !key.startsWith('settings.search.keywords.'))
      .filter(([key]) => /\bbuil[dt]/i.test(flatEn[key] ?? '') && !/pack/i.test(flatEn[key] ?? ''))
      .filter(([, value]) => typeof value === 'string' && /сборк/i.test(value))
      .map(([key]) => key);
    expect(offenders).toEqual([]);
  });

  // The same collision for the profile: «Эта сборка не запустится» said a MODPACK would not
  // launch where the English says the instance won't (plan §5b V2, carried from V1). An instance
  // is «профиль» (D14). Found the same way: an instance that is not a pack. Search keywords exempt.
  it('never says «сборка» for an instance', () => {
    const offenders = Object.entries(flatRu)
      .filter(([key]) => !key.startsWith('settings.search.keywords.'))
      .filter(([key]) => /\binstance/i.test(flatEn[key] ?? '') && !/pack/i.test(flatEn[key] ?? ''))
      .filter(([, value]) => typeof value === 'string' && /сборк/i.test(value))
      .map(([key]) => key);
    expect(offenders).toEqual([]);
  });

  // A version reaches a string already formatted (`displayVersion`: «v» only before a leading
  // digit). A «v» written into the string itself doubled a version that has its own — «vv2.1.0»
  // (plan §5b V2) — so no string glues a «v» to an argument.
  it('never glues a «v» to an argument', () => {
    const glued = /(^|[^\p{L}])v\{/u;
    const offenders = [
      ...Object.entries(flatEn).map(([k, v]) => [`en:${k}`, v] as const),
      ...Object.entries(flatRu).map(([k, v]) => [`ru:${k}`, v] as const),
    ]
      .filter(([, value]) => typeof value === 'string' && glued.test(value))
      .map(([key]) => key);
    expect(offenders).toEqual([]);
  });
});
