import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

// A numeric ICU argument — `{n, plural, …}`, `{n, selectordinal, …}` or
// `{n, number}` — must reach $t() as a NUMBER. intl-messageformat runs it
// through Number() to pick the plural category and to render `#`; a
// pre-formatted string (e.g. `(n).toLocaleString()`) becomes NaN, and
// Intl.NumberFormat('ru') renders NaN as the literal "не число" — which is
// exactly how this shipped on the modpack cards.
//
// Pre-formatting is never needed and is actively wrong: these ICU types group
// the digits themselves using the UI locale, whereas a bare toLocaleString()
// uses the OS locale, so an English UI on a Russian Windows printed "12 345".
//
// Note that a bare `{n}` does NOT format — it stringifies. A counter that
// should carry a thousands separator has to be typed `{n, number}` in the
// dictionary; that is why this guard covers `number` too.

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

function sourceFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) sourceFiles(full, acc);
    else if (/\.(svelte|ts)$/.test(entry.name)) acc.push(full);
  }
  return acc;
}

/** key -> EVERY numeric ICU argument name, unioned across BOTH locales. */
function numericArgKeys(): Record<string, string[]> {
  const out: Record<string, Set<string>> = {};
  for (const dict of [flatten(en as Json), flatten(ru as Json)]) {
    for (const [key, value] of Object.entries(dict)) {
      for (const m of value.matchAll(/\{\s*(\w+)\s*,\s*(?:plural|selectordinal|number)\s*[,}]/g)) {
        (out[key] ??= new Set()).add(m[1]);
      }
    }
  }
  return Object.fromEntries(Object.entries(out).map(([k, v]) => [k, [...v]]));
}

// Expressions that are strings (or stringify) rather than numbers.
// `.toFixed()` is here for the same reason `.toLocaleString()` is: it returns a
// STRING with an English decimal point, which a `{n, number}` argument then
// renders verbatim — so "1.5 МБ/с" landed next to formatSize's "1,5 МБ" on the
// same upload line. Rounding belongs in the ICU skeleton, not the caller.
const STRINGY = /\.toLocaleString\s*\(|\.toFixed\s*\(|\bString\s*\(|^['"`]/;

describe('ICU numeric arguments are numbers', () => {
  const keys = numericArgKeys();
  const files = sourceFiles('src');

  // Collect every `<arg>:` value found near such a key's call site. Bounded
  // look-ahead rather than brace matching: $t() calls in this codebase are
  // short object literals, and over-reading only risks a false positive,
  // which a reviewer sees rather than a silent miss.
  const callSites: { file: string; key: string; expr: string }[] = [];
  for (const file of files) {
    const src = readFileSync(file, 'utf8');
    for (const [key, args] of Object.entries(keys)) {
      let at = src.indexOf(`'${key}'`);
      while (at !== -1) {
        const nearby = src.slice(at, at + 400);
        for (const arg of args) {
          const m = new RegExp(`\\b${arg}\\s*:\\s*([^,\\n}]+)`).exec(nearby);
          if (m) callSites.push({ file, key, expr: m[1].trim() });
        }
        at = src.indexOf(`'${key}'`, at + 1);
      }
    }
  }

  it('finds numeric-argument keys and their call sites', () => {
    // Guards the guard: a regex that stops matching would otherwise make every
    // assertion below vacuously true.
    expect(Object.keys(keys).length).toBeGreaterThan(20);
    expect(callSites.length).toBeGreaterThan(20);
  });

  it('no call site passes a pre-formatted string as the numeric argument', () => {
    const offenders = callSites
      .filter(({ expr }) => STRINGY.test(expr))
      .map(({ file, key, expr }) => `${file} :: ${key} :: ${expr}`);
    expect(offenders).toEqual([]);
  });
});
