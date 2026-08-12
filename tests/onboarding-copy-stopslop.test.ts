import { describe, expect, it } from 'vitest';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

// Stop-slop regression guards for the onboarding copy.
//
// EM-DASH BAN IS EN-ONLY: in English prose an em-dash (—) is the classic
// AI-slop tell, so the EN onboarding strings must not use it. In Russian,
// тире (—) is standard, often grammatically required punctuation
// («Профиль — это…»), so the RU copy is intentionally exempt from this check.
//
// MACHINE NOTATION IS BANNED IN BOTH: a bare " = " ("Switching instance =
// switching install") reads as machine notation in any language and is not a
// substitute for prose.
//
// CURLY APOSTROPHE IS BANNED IN EN: U+2019 in prose is another
// paste-from-a-doc tell; the EN UI uses straight ' throughout. RU is exempt
// like the em-dash rule.

function leaves(obj: unknown, path = ''): Array<[string, string]> {
  if (typeof obj === 'string') return [[path, obj]];
  if (obj && typeof obj === 'object') {
    return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
      leaves(v, path ? `${path}.${k}` : k),
    );
  }
  return [];
}

const enOnboarding = leaves((en as Record<string, unknown>).onboarding, 'onboarding');
const ruOnboarding = leaves((ru as Record<string, unknown>).onboarding, 'onboarding');

describe('onboarding copy — stop-slop guards', () => {
  it('has no em-dash in any EN onboarding string', () => {
    const offenders = enOnboarding.filter(([, v]) => v.includes('—')).map(([k]) => k);
    expect(offenders).toEqual([]);
  });

  it('has no " = " machine notation in EN or RU onboarding strings', () => {
    const offenders = [...enOnboarding, ...ruOnboarding]
      .filter(([, v]) => v.includes(' = '))
      .map(([k]) => k);
    expect(offenders).toEqual([]);
  });

  it('has no curly apostrophes in EN onboarding strings (U+2019)', () => {
    const offenders = enOnboarding.filter(([, v]) => v.includes('’')).map(([k]) => k);
    expect(offenders).toEqual([]);
  });
});
