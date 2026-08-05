// Whether the AI pre-fill could run at all, and if not, which single thing is
// missing — so LocalizationModal can leave its triggers on screen and NAME the
// gap instead of hiding a feature the user then never learns exists.
//
// Advisory, deliberately. The authoritative preflight is
// `l10n::prefill::run::resolve_provider` on the Rust side, re-run by
// `l10nPrefillEstimate` the moment the estimate dialog opens. A disagreement
// here therefore degrades to "the button was live, the dialog explained why
// not" — today's behaviour — and never to a wrong action. That bounded blast
// radius is what makes this a small pure function rather than a new IPC
// command.
//
// It mirrors ONE Rust rule: `AiProvider::needs_key()` in
// `src-tauri/src/l10n/prefill/provider.rs` — hosted providers need a stored
// key, a local server needs none. `NEEDS_KEY` is a Record over the union, so
// adding a provider is a compile error here rather than a wrong tooltip at
// runtime.
//
// What it deliberately does NOT mirror: a local provider with a blank model
// name also cannot run (`resolve_model` refuses, because nothing can enumerate
// a local server's models offline). That is left to the estimate dialog. The
// button would be answering "is the feature set up?", and a local setup with
// no model IS set up — it is misconfigured, which is a different sentence and
// belongs where the run actually fails.

import type { AiProvider } from '$lib/ipc/bindings';

/** `ready` — a run could start. The other two are both fixable in Settings. */
export type PrefillReadiness = 'ready' | 'no_consent' | 'no_key';

const NEEDS_KEY: Record<AiProvider, boolean> = {
  anthropic: true,
  gemini: true,
  groq: true,
  local: false,
};

/**
 * Consent first: a user who has not permitted the feature should be told to
 * permit it, not sent off to paste a credential for something still switched
 * off.
 */
export function prefillReadiness(facts: {
  consent: boolean;
  provider: AiProvider;
  keyStored: boolean;
}): PrefillReadiness {
  if (!facts.consent) return 'no_consent';
  if (NEEDS_KEY[facts.provider] && !facts.keyStored) return 'no_key';
  return 'ready';
}

/** Whether asking the keyring is worth an IPC round trip for `provider`. */
export function needsStoredKey(provider: AiProvider): boolean {
  return NEEDS_KEY[provider];
}
