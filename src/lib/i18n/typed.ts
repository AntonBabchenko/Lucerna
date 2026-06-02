// A thin typed wrapper over svelte-i18n's message formatter store.
// `$t('a.b.c', { values })` is reactive (re-renders on locale change)
// AND compile-time checked against the generated key union.
import { derived } from 'svelte/store';
import { _ } from 'svelte-i18n';
import type { TranslationKey } from './keys.generated';

// Mirrors svelte-i18n's InterpolationValues (not publicly exported by the package).
type InterpolationValues = Record<string, string | number | boolean | Date | null | undefined>;

export const t = derived(
  _,
  (format) =>
    (key: TranslationKey, values?: InterpolationValues): string =>
      // `format()`'s type includes `undefined`, but svelte-i18n returns the
      // key itself for a missing message at runtime; `?? key` just satisfies
      // the `: string` return type.
      format(key, values ? { values } : undefined) ?? key,
);
