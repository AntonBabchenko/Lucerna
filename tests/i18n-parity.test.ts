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

  it('ICU argument placeholders match between en and ru', () => {
    const mismatches = Object.keys(flatEn)
      .filter((k) => k in flatRu)
      .map((k) => ({ k, en: argPlaceholders(flatEn[k]), ru: argPlaceholders(flatRu[k]) }))
      .filter(({ en, ru }) => en.join(',') !== ru.join(','))
      .map(({ k, en, ru }) => `${k} (en:[${en}] ru:[${ru}])`);
    expect(mismatches).toEqual([]);
  });
});
