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

function bodyKey(status: number): TranslationKey {
  if (status === 401 || status === 403) return 'errors.l10nPrefillProvider.rejectedKey';
  if (status === 404) return 'errors.l10nPrefillProvider.unknownModel';
  if (status === 429) return 'errors.l10nPrefillProvider.rateLimited';
  if (status >= 500 && status <= 599) return 'errors.l10nPrefillProvider.providerTrouble';
  if (status === 0) return 'errors.l10nPrefillProvider.unreadable';
  return 'errors.l10nPrefillProvider.other';
}

/**
 * One sentence for the failure, by status, plus where the full response is.
 * `context` names the request: a connection test is not a translation run.
 */
export function describeProviderFailure(e: ProviderFailure, context: 'test' | 'run'): string {
  const translate = get(t);
  const name = providerDisplayName(e.provider);
  const request = translate(
    context === 'test'
      ? 'errors.l10nPrefillProvider.requestTest'
      : 'errors.l10nPrefillProvider.requestRun',
  );
  const body = translate(bodyKey(e.status), { name, request, status: e.status });
  return `${body} ${translate('errors.l10nPrefillProvider.logsTail')}`;
}

/** `describeProviderFailure` for a provider error; `null` for any other kind. */
export function providerFailureOrNull(e: IpcError, context: 'test' | 'run'): string | null {
  return e.kind === 'l10n_prefill_provider' ? describeProviderFailure(e, context) : null;
}
