// The language picker's options, built from the bundle's catalog: every
// language named in itself, and a partial one with how much is translated, so
// nobody picks it expecting a whole UI. Pure, so the label rule is testable
// without rendering the panel or planting a locale file.
import type { Translate } from '$lib/i18n';
import type { LocaleInfo } from '$lib/i18n/catalog';

export interface LanguageOption {
  value: string;
  label: string;
}

export function languageLabel(info: LocaleInfo, t: Translate): string {
  if (info.percent >= 100) return info.endonym;
  return t('settings.general.appearance.languagePartial', {
    name: info.endonym,
    percent: info.percent,
  });
}

export function languageOptions(
  catalog: readonly LocaleInfo[],
  systemLabel: string,
  t: Translate,
): LanguageOption[] {
  return [
    { value: 'system', label: systemLabel },
    ...catalog.map((info) => ({ value: info.code, label: languageLabel(info, t) })),
  ];
}
