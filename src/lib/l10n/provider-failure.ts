// What a hosted AI provider's non-2xx answer means. A bad key, a wrong model
// name, a rate limit and provider trouble are four different remedies, so the
// status code picks the sentence; the provider is named by its display name;
// the body is never rendered (the error's class is `transport` — see
// `ERROR_CLASS` in format-error.ts).
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { AiProvider, Error as IpcError } from '$lib/ipc/bindings';

export type ProviderFailure = Extract<IpcError, { kind: 'l10n_prefill_provider' }>;

const PROVIDER_NAME_KEY: Record<AiProvider, TranslationKey> = {
  anthropic: 'settings.aiTranslation.providerAnthropic',
  gemini: 'settings.aiTranslation.providerGemini',
  groq: 'settings.aiTranslation.providerGroq',
  local: 'settings.aiTranslation.providerLocal',
};

/** The wire field is a plain string; only a known id gets a display name. */
export function isAiProvider(id: string): id is AiProvider {
  return Object.hasOwn(PROVIDER_NAME_KEY, id);
}

export function providerDisplayName(id: string): string {
  return isAiProvider(id) ? get(t)(PROVIDER_NAME_KEY[id]) : id;
}

/**
 * One sentence for the failure, by status, plus where the full response is.
 * `context` names the request: a connection test is not a translation run.
 */
export function describeProviderFailure(e: ProviderFailure, context: 'test' | 'run'): string {
  // RED STUB (push 1): one sentence for every status, the raw provider id.
  const translate = get(t);
  const request = translate(
    context === 'test'
      ? 'errors.l10nPrefillProvider.requestTest'
      : 'errors.l10nPrefillProvider.requestRun',
  );
  const body = translate('errors.l10nPrefillProvider.other', {
    name: e.provider,
    request,
    status: e.status,
  });
  return `${body} ${translate('errors.l10nPrefillProvider.logsTail')}`;
}

/** `describeProviderFailure` for a provider error; `null` for any other kind. */
export function providerFailureOrNull(e: IpcError, context: 'test' | 'run'): string | null {
  return e.kind === 'l10n_prefill_provider' ? describeProviderFailure(e, context) : null;
}
