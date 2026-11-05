import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { IntlMessageFormat } from 'intl-messageformat';
import { canonicalLocaleCode } from '../../src/lib/i18n/catalog';

type Json = Record<string, unknown>;

/** A dictionary's nested keys as dotted paths; a non-object leaf is kept as found. */
export function flatten(obj: Json, prefix = ''): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object' && !Array.isArray(v))
      Object.assign(out, flatten(v as Json, path));
    else out[path] = v;
  }
  return out;
}

export interface UiDictionary {
  /** The file name, e.g. `zh_Hans.json` — what a failure names. */
  file: string;
  /** The canonical code the bundle keys it by, e.g. `zh-Hans`. */
  code: string;
  flat: Record<string, unknown>;
}

const LOCALES_DIR = join('src', 'lib', 'i18n', 'locales');

/**
 * Every UI dictionary the bundle ships: `src/lib/i18n/locales/*.json`, found the
 * way `dictionaries.ts` finds them (a glob, not a list), so a language Weblate
 * adds is checked the day it lands.
 */
export function uiDictionaries(): UiDictionary[] {
  return readdirSync(LOCALES_DIR)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((file) => ({
      file,
      code: canonicalLocaleCode(file.slice(0, -'.json'.length)),
      flat: flatten(JSON.parse(readFileSync(join(LOCALES_DIR, file), 'utf8')) as Json),
    }));
}

/**
 * ICU element types as `@formatjs/icu-messageformat-parser` numbers them (its
 * `TYPE` enum, which intl-messageformat does not re-export). A guard in the
 * test that uses them pins these numbers on a sample message.
 */
export const ICU = {
  literal: 0,
  argument: 1,
  number: 2,
  date: 3,
  time: 4,
  select: 5,
  plural: 6,
  pound: 7,
  tag: 8,
} as const;

export interface IcuElement {
  type: number;
  value?: string;
  options?: Record<string, { value: IcuElement[] }>;
  pluralType?: 'cardinal' | 'ordinal';
  children?: IcuElement[];
}

/** The message as `$t` parses it (`typed.ts`: same constructor, same options); throws as it would. */
export function parseMessage(message: string, code: string): IcuElement[] {
  return new IntlMessageFormat(message, code, undefined, {
    ignoreTag: true,
  }).getAst() as IcuElement[];
}
