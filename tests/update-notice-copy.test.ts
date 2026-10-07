import { describe, expect, it } from 'vitest';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

// The post-update notice sits right beside its button: a notice that ends in
// the button's own words says them twice ("Updated to 0.26.0 — see what's new"
// next to "What's new").
describe('the post-update notice', () => {
  for (const [code, file] of [
    ['en', en],
    ['ru', ru],
  ] as const) {
    it(`does not repeat its button in ${code}`, () => {
      const notice = file.page.whatsNew.toast.toLowerCase();
      const button = file.page.whatsNew.actionLabel.toLowerCase();
      expect(notice).not.toContain(button);
    });
  }
});
