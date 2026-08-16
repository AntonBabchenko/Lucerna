import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// `new Date(x).toLocaleString()` with no locale argument silently follows the
// OS, so a Russian UI on an English Windows rendered its dates in English and
// the in-app language switch did not move them. The fix is one grep-able home:
// $lib/format/date-time.ts, whose helpers take the svelte-i18n locale
// explicitly. This guard is why a future call site cannot quietly go back.
//
// NOT in scope here: a NUMERIC toLocaleString(). That one is wrong for a
// different reason (the dictionary must type the argument `{n, number}` and
// the caller must pass a raw number) and is enforced by
// tests/i18n-plural-args.test.ts. With both landed, src/ contains no direct
// toLocale*String call outside the module below — which is what makes a flat
// "zero offenders" assertion possible at all.
const DATE_TIME_MODULE = join('src', 'lib', 'format', 'date-time.ts');

// Deliberately NOT /g: a global regex carries lastIndex across .test() calls
// and would skip every other file.
const TO_LOCALE = /\.toLocale(?:Date|Time)?String\s*\(/;

function sourceFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) sourceFiles(full, acc);
    else if (/\.(svelte|ts)$/.test(entry.name)) acc.push(full);
  }
  return acc;
}

describe('dates follow the app locale, not the OS', () => {
  const files = sourceFiles('src');

  it('is exempting a module that exists and actually formats dates', () => {
    // Guards the guard: if date-time.ts were renamed or gutted, the exemption
    // would cover nothing and the scan below would still pass vacuously.
    expect(files).toContain(DATE_TIME_MODULE);
    expect(TO_LOCALE.test(readFileSync(DATE_TIME_MODULE, 'utf8'))).toBe(true);
  });

  it('no source file outside $lib/format/date-time.ts calls toLocale*String', () => {
    const offenders = files
      .filter((f) => f !== DATE_TIME_MODULE)
      .filter((f) => TO_LOCALE.test(readFileSync(f, 'utf8')))
      .sort();
    expect(offenders).toEqual([]);
  });
});
