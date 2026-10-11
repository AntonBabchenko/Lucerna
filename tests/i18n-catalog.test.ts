import { describe, expect, it } from 'vitest';
import {
  canonicalLocaleCode,
  localeEndonym,
  stringAt,
  translatedPercent,
} from '../src/lib/i18n/catalog';

describe('canonicalLocaleCode', () => {
  it('turns a Weblate POSIX file name into a BCP-47 tag', () => {
    expect(canonicalLocaleCode('pt_BR')).toBe('pt-BR');
    expect(canonicalLocaleCode('zh_Hans')).toBe('zh-Hans');
  });

  it('keeps a canonical tag as it is and canonicalises case', () => {
    expect(canonicalLocaleCode('en')).toBe('en');
    expect(canonicalLocaleCode('ru')).toBe('ru');
    expect(canonicalLocaleCode('pt-br')).toBe('pt-BR');
  });

  it('returns a name that is not a language tag unchanged', () => {
    expect(canonicalLocaleCode('not a tag!')).toBe('not a tag!');
    expect(canonicalLocaleCode('')).toBe('');
  });
});

describe('localeEndonym', () => {
  it('names a language in that language, first letter capitalised', () => {
    expect(localeEndonym('ru')).toBe('Русский');
    expect(localeEndonym('fr')).toBe('Français');
    expect(localeEndonym('de')).toBe('Deutsch');
    expect(localeEndonym('en')).toBe('English');
    expect(localeEndonym('pt-BR')).toBe('Português (Brasil)');
  });

  it('falls back to the code when Intl has no name for it', () => {
    expect(localeEndonym('xx')).toBe('xx');
    expect(localeEndonym('not a tag!')).toBe('not a tag!');
  });
});

describe('translatedPercent', () => {
  const source = { a: 'A', b: { c: 'C', d: 'D' }, e: 'E' };

  it('is 100 for the source against itself', () => {
    expect(translatedPercent(source, source)).toBe(100);
  });

  it('counts a missing key and an empty string as untranslated', () => {
    expect(translatedPercent({ a: 'а', b: { c: '' } }, source)).toBe(25);
  });

  it('counts a value equal to the English one as translated', () => {
    expect(translatedPercent({ a: 'A', b: { c: 'C', d: 'D' }, e: 'E' }, source)).toBe(100);
  });

  it('ignores keys the source does not have', () => {
    expect(translatedPercent({ a: 'а', z: 'z', b: { y: 'y' } }, source)).toBe(25);
  });

  it('rounds down with integer arithmetic, so 29 of 100 is 29, not 28', () => {
    const keys = (n: number, v: string) =>
      Object.fromEntries(Array.from({ length: n }, (_, i) => [`k${i}`, v]));
    expect(translatedPercent(keys(29, 'y'), keys(100, 'x'))).toBe(29);
    // 100 of 101 is 99.0…%: never shown as 100.
    expect(translatedPercent(keys(100, 'y'), keys(101, 'x'))).toBe(99);
  });

  it('is 0 for a dictionary that is not an object, and for an empty source', () => {
    expect(translatedPercent(undefined, source)).toBe(0);
    expect(translatedPercent(source, {})).toBe(0);
  });
});

describe('stringAt', () => {
  it('reads a nested string and refuses anything else', () => {
    expect(stringAt({ a: { b: 'x' } }, ['a', 'b'])).toBe('x');
    expect(stringAt({ a: { b: 1 } }, ['a', 'b'])).toBeNull();
    expect(stringAt({ a: {} }, ['a', 'b'])).toBeNull();
    expect(stringAt(null, ['a'])).toBeNull();
  });
});
