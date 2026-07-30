// Dispatch of an inbound LaunchIntent — a desktop shortcut's argv, or a
// `lucerna://` URL the OS handed us (directly at cold start, or forwarded by the
// single-instance guard from a second launch).
//
// Trust model (design spec §2): argv is trusted and may launch the game; a URL
// is untrusted and may only open the import confirmation UI. That split is
// enforced in Rust (`cli::parse` / `deeplink::parse_import_url`) — this module
// only routes an already-typed intent to the page's two handlers. Kept free of
// Svelte state so it stays unit-testable.

import type { LaunchIntent, QuickPlay } from '$lib/ipc/bindings';

export type IntentHandlers = {
  onUrl: (url: string) => void | Promise<void>;
  onLaunch: (instanceId: string, quickPlay: QuickPlay | null) => void | Promise<void>;
};

export async function dispatchIntent(
  intent: LaunchIntent | null,
  handlers: IntentHandlers,
): Promise<void> {
  if (!intent) return;
  if (intent.kind === 'open_url') {
    await handlers.onUrl(intent.url);
    return;
  }
  await handlers.onLaunch(intent.instance, intent.quick_play);
}
