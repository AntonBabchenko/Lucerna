import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { loadersByCode } from '../src/lib/changelog/locales';
import { canonicalLocaleCode } from '../src/lib/i18n/catalog';

// A locale file's name IS its language: the bundle canonicalises it (Weblate
// writes pt_BR, the app needs pt-BR). A name that is no language tag would
// reach svelte-i18n, <html lang> and Intl as garbage, and two spellings of one
// language would silently shadow each other — so both fail here, before a
// build ships them.
function problems(names: readonly string[]): string[] {
  const out: string[] = [];
  const seen = new Map<string, string>();
  for (const name of names) {
    let valid = name !== '';
    try {
      Intl.getCanonicalLocales(name.replace(/_/g, '-'));
    } catch {
      valid = false;
    }
    if (!valid) {
      out.push(`${name}: not a language tag`);
      continue;
    }
    const code = canonicalLocaleCode(name);
    const earlier = seen.get(code);
    if (earlier !== undefined) out.push(`${name}: same language as ${earlier} (${code})`);
    seen.set(code, name);
  }
  return out;
}

function baseNames(dir: string, ext: string): string[] {
  return readdirSync(dir)
    .filter((f) => f.endsWith(ext))
    .map((f) => f.slice(0, -ext.length));
}

describe('locale file names', () => {
  it('flags a name that is no language tag, and two spellings of one language', () => {
    expect(problems(['en', 'pt_BR', 'zh-Hans'])).toEqual([]);
    expect(problems(['en', 'not a tag!'])).toEqual(['not a tag!: not a language tag']);
    expect(problems(['pt-BR', 'pt_BR'])).toEqual(['pt_BR: same language as pt-BR (pt-BR)']);
    // An alias is the same language too: iw is the old code for Hebrew.
    expect(problems(['he', 'iw'])).toEqual(['iw: same language as he (he)']);
  });

  it('every UI dictionary is named by a distinct language tag', () => {
    const names = baseNames(join('src', 'lib', 'i18n', 'locales'), '.json');
    expect(names).toContain('en');
    expect(problems(names)).toEqual([]);
  });

  it('every changelog translation is named by a distinct language tag', () => {
    const names = baseNames(join('src', 'lib', 'changelog', 'locales'), '.md');
    expect(names).toContain('ru');
    expect(problems(names)).toEqual([]);
  });

  it('keys a Weblate-named changelog translation by the code the UI locale uses', () => {
    const load = async () => '';
    const loaders = loadersByCode({ './locales/pt_BR.md': load, './locales/ru.md': load });
    expect(Object.keys(loaders).sort()).toEqual(['pt-BR', 'ru']);
    expect(loaders['pt-BR']).toBe(load);
  });
});
