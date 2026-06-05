import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { ExplanationLevel } from '$lib/ipc/bindings';

/**
 * Resolve a base (Advanced) translation key to the variant for the given level.
 * Advanced → the base key unchanged. Basic → the sibling leaf with a `Basic`
 * suffix (e.g. `…welcome.body` → `…welcome.bodyBasic`). The cast is safe: a
 * parity test (tests/explanation-basic-parity.test.ts) asserts every adaptive
 * base key has its `*Basic` sibling in both locales, so the resolved key always
 * exists.
 */
export function explainKey(base: TranslationKey, level: ExplanationLevel): TranslationKey {
  return (level === 'advanced' ? base : `${base}Basic`) as TranslationKey;
}
