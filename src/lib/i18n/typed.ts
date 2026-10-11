// A thin typed translator over svelte-i18n's dictionaries, with its own
// message-formatter cache. `$t('a.b.c', { values })` is reactive (re-renders
// on a locale or dictionary change) AND compile-time checked against the
// generated key union.
//
// Why not svelte-i18n's `_`: it memoizes formatters on the MESSAGE text alone
// and fixes the locale at the first call, so two locales that share a message
// share one formatter. A partial community translation shares every message it
// lacks with English (svelte-i18n falls back to it), so a `{n, number}` first
// rendered in German kept German digit grouping after a switch to English.
// Here the cache key is the locale AND the message. Lookup, with its fallback
// chain to English, is still svelte-i18n's (`json`).

import { IntlMessageFormat } from 'intl-messageformat';
import { derived } from 'svelte/store';
import { json, locale } from 'svelte-i18n';
import type { TranslationKey } from './keys.generated';

// Mirrors svelte-i18n's InterpolationValues (not publicly exported by the package).
type InterpolationValues = Record<string, string | number | boolean | Date | null | undefined>;

/** The type of the translator function returned by the `$t` store. */
export type Translate = (key: TranslationKey, values?: InterpolationValues) => string;

const formatters = new Map<string, IntlMessageFormat>();

function formatterFor(message: string, loc: string): IntlMessageFormat {
  const cacheKey = `${loc}\u0000${message}`;
  let formatter = formatters.get(cacheKey);
  if (formatter === undefined) {
    // ignoreTag as svelte-i18n sets it: a `<b>` in a message is text, not a
    // rich-text tag. No named formats: the dictionaries use ICU skeletons only.
    formatter = new IntlMessageFormat(message, loc, undefined, { ignoreTag: true });
    formatters.set(cacheKey, formatter);
  }
  return formatter;
}

export const t = derived(
  [json, locale],
  ([lookup, current]) =>
    (key: TranslationKey, values?: InterpolationValues): string => {
      // Look up and format in the same locale this translator was made for.
      const message: unknown = lookup(key, current ?? undefined);
      // No message anywhere in the fallback chain: the key itself, as svelte-i18n renders it.
      if (typeof message !== 'string' || message === '') return key;
      if (values === undefined || current == null) return message;
      try {
        return String(formatterFor(message, current).format(values));
      } catch (e) {
        // svelte-i18n's own contract: a message that cannot be formatted is
        // shown as written and reported, never thrown into a render.
        console.warn(`[i18n] message "${key}" could not be formatted:`, e);
        return message;
      }
    },
);
