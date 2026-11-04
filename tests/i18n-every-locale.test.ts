import { describe, expect, it } from 'vitest';
import {
  ICU,
  type IcuElement,
  parseMessage,
  type UiDictionary,
  uiDictionaries,
} from './test-utils/dictionaries';

// Every UI dictionary, not only ru. Community languages arrive through Weblate,
// partly translated, and no reviewer reads their ICU: #537 shipped to review
// with a plural whose `other` was translated and a message that had lost `{mc}`.
// tests/i18n-parity.test.ts still holds ru to FULL parity with en; this file
// holds every dictionary, en and ru included, to what keeps a message working.
//
// The rules read the message as `$t` parses it, not a regex over its text. A
// regex took `'{modName}'` for an argument where ICU reads quoted text, and the
// English UI showed «{modName} was bundled inside the .mrpack…» for that reason.

const dictionaries = uiDictionaries();
const en = dictionaries.find((d) => d.code === 'en') as UiDictionary;
const translations = dictionaries.filter((d) => d.code !== 'en');

type Parsed = { d: UiDictionary; key: string; ast: IcuElement[] };

/** Every string message of `ds` that parses; the ones that do not are rule 3's business. */
function parsedMessages(ds: readonly UiDictionary[]): Parsed[] {
  const out: Parsed[] = [];
  for (const d of ds) {
    for (const [key, value] of Object.entries(d.flat)) {
      if (typeof value !== 'string') continue;
      try {
        out.push({ d, key, ast: parseMessage(value, d.code) });
      } catch {
        // reported by «every message parses as $t parses it»
      }
    }
  }
  return out;
}

/** Each element of the AST, depth first, through plural/select branches and tags. */
function* walk(els: readonly IcuElement[]): Generator<IcuElement> {
  for (const el of els) {
    yield el;
    for (const option of Object.values(el.options ?? {})) yield* walk(option.value);
    if (el.children) yield* walk(el.children);
  }
}

const ARGUMENT_TYPES: readonly number[] = [
  ICU.argument,
  ICU.number,
  ICU.date,
  ICU.time,
  ICU.select,
  ICU.plural,
];

/** The argument names a message reads, anywhere in it, as a sorted set. */
function argumentNames(ast: readonly IcuElement[]): string[] {
  const names = new Set<string>();
  for (const el of walk(ast)) if (ARGUMENT_TYPES.includes(el.type) && el.value) names.add(el.value);
  return [...names].sort();
}

const enAst = new Map(parsedMessages([en]).map((p) => [p.key, p.ast]));
const translated = parsedMessages(translations).filter((p) => enAst.has(p.key));

describe('every UI dictionary keeps its messages working', () => {
  it('reads every shipped dictionary and parses the way it expects', () => {
    // Guards the guard: a broken glob or a renumbered parser would make every
    // rule below pass on nothing.
    expect(dictionaries.map((d) => d.code)).toEqual(
      expect.arrayContaining(['en', 'ru', 'zh-Hans']),
    );
    expect(translated.length).toBeGreaterThan(4000);
    const sample = parseMessage(
      '{a} {n, plural, one {#} other {{b}}} {k, select, x {X} other {O}}',
      'en',
    );
    expect(sample.map((el) => el.type)).toEqual([
      ICU.argument,
      ICU.literal,
      ICU.plural,
      ICU.literal,
      ICU.select,
    ]);
    expect(argumentNames(sample)).toEqual(['a', 'b', 'k', 'n']);
  });

  it('has no key en lacks in a translation', () => {
    const extra = translations.flatMap((d) =>
      Object.keys(d.flat)
        .filter((key) => !(key in en.flat))
        .map((key) => `${d.file}:${key}`),
    );
    expect(extra).toEqual([]);
  });

  it('has a non-empty string for every key', () => {
    const bad = dictionaries.flatMap((d) =>
      Object.entries(d.flat)
        .filter(([, value]) => typeof value !== 'string' || !value.trim())
        .map(([key]) => `${d.file}:${key}`),
    );
    expect(bad).toEqual([]);
  });

  it('parses every message as $t parses it', () => {
    // A translated `other` (MISSING_OTHER_CLAUSE) or an unbalanced brace: $t
    // throws on the message, and the screen shows its key instead.
    const errors: string[] = [];
    for (const d of dictionaries) {
      for (const [key, value] of Object.entries(d.flat)) {
        if (typeof value !== 'string') continue;
        try {
          parseMessage(value, d.code);
        } catch (e) {
          errors.push(`${d.file}:${key} — ${(e as Error).message}`);
        }
      }
    }
    expect(errors).toEqual([]);
  });

  it('never shows a brace as text', () => {
    // ICU reads an ASCII apostrophe before `{` as the start of quoted text, so
    // `'{name}'` prints «{name}» instead of the value. No message in this UI
    // means a literal brace; a name in quotes takes “…” or «…».
    const quoted = parsedMessages(dictionaries)
      .filter(({ ast }) =>
        [...walk(ast)].some((el) => el.type === ICU.literal && /[{}]/.test(el.value ?? '')),
      )
      .map(({ d, key }) => `${d.file}:${key}`);
    expect(quoted).toEqual([]);
  });

  it("reads exactly en's arguments in every translated message", () => {
    // A dropped argument loses its value (#537: «{mc}»); an added one has no
    // value to read, and intl-messageformat throws when it formats.
    const mismatches = translated
      .map(({ d, key, ast }) => ({
        d,
        key,
        own: argumentNames(ast),
        want: argumentNames(enAst.get(key) ?? []),
      }))
      .filter(({ own, want }) => own.join(',') !== want.join(','))
      .map(({ d, key, own, want }) => `${d.file}:${key} reads [${own}], en reads [${want}]`);
    expect(mismatches).toEqual([]);
  });

  // Not checked: a `select`'s option keys. No message in any dictionary uses
  // select today; the first one should bring the rule that its translated
  // option keys are en's (a translated key is never matched).

  it('keys every plural by a CLDR category or an exact number', () => {
    // `few {…}` is chosen by Intl.PluralRules; a word it does not know (a
    // translated keyword) is a branch nothing ever selects.
    const CLDR = new Set(['zero', 'one', 'two', 'few', 'many', 'other']);
    const bad = parsedMessages(dictionaries).flatMap(({ d, key, ast }) =>
      [...walk(ast)]
        .filter((el) => el.type === ICU.plural)
        .flatMap((el) =>
          Object.keys(el.options ?? {})
            .filter((k) => !CLDR.has(k) && !/^=\d+$/.test(k))
            .map((k) => `${d.file}:${key} plural ${el.value} has «${k}»`),
        ),
    );
    expect(bad).toEqual([]);
  });
});
