import { describe, expect, it } from 'vitest';
import type { LocaleInfo } from '../src/lib/i18n/catalog';
import { languageLabel, languageOptions } from '../src/lib/settings/language-options';

// A stand-in translator that shows the key and its values, so the test pins
// which message is used and with what — not the English wording.
const t = ((key: string, values?: Record<string, unknown>) =>
  values ? `${key}|${values.name}|${values.percent}` : key) as never;

const de = (percent: number): LocaleInfo => ({ code: 'de', endonym: 'Deutsch', percent });

describe('languageLabel', () => {
  it('names a complete language by its endonym alone', () => {
    expect(languageLabel(de(100), t)).toBe('Deutsch');
  });

  it('adds how much is translated for a partial language', () => {
    expect(languageLabel(de(42), t)).toBe('settings.general.appearance.languagePartial|Deutsch|42');
    expect(languageLabel(de(99), t)).toBe('settings.general.appearance.languagePartial|Deutsch|99');
    expect(languageLabel(de(0), t)).toBe('settings.general.appearance.languagePartial|Deutsch|0');
  });
});

describe('languageOptions', () => {
  it('puts System first, then one option per bundled language', () => {
    const opts = languageOptions(
      [{ code: 'en', endonym: 'English', percent: 100 }, de(42)],
      'System — currently English',
      t,
    );
    expect(opts.map((o) => o.value)).toEqual(['system', 'en', 'de']);
    expect(opts[0].label).toBe('System — currently English');
    expect(opts[1].label).toBe('English');
    expect(opts[2].label).toBe('settings.general.appearance.languagePartial|Deutsch|42');
  });
});
