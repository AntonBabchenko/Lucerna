import { describe, expect, it } from 'vitest';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

// The Modpacks tour's filter step told the user to click a «Фильтры» button that has been gone
// since the filters moved inline, and named the reset «Очистить всё» / «Сбросить всё» while the
// button reads «Сбросить все» (2026-10-02 regression, O5). The step now quotes the reset by its
// button's own label, in both registers, so the two cannot drift apart again.
describe('Modpacks tour — the filter step describes the bar as it is', () => {
  const cases = [
    {
      lang: 'en',
      step: en.onboarding.contextual.modpacks.searchSortFilter,
      reset: `"${en.browse.filter.clearAll}"`,
      gone: '"Filters"',
    },
    {
      lang: 'ru',
      step: ru.onboarding.contextual.modpacks.searchSortFilter,
      reset: `«${ru.browse.filter.clearAll}»`,
      gone: '«Фильтры»',
    },
  ];

  for (const c of cases) {
    it(`${c.lang}: names the reset by its button's label`, () => {
      expect(c.step.body).toContain(c.reset);
      expect(c.step.bodyBasic).toContain(c.reset);
    });

    it(`${c.lang}: points at no Filters button`, () => {
      expect(c.step.body).not.toContain(c.gone);
      expect(c.step.bodyBasic).not.toContain(c.gone);
    });
  }
});
