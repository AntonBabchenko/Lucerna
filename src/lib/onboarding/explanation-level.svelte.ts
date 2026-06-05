// Explanation detail level runtime — bridges the persisted GeneralSettings
// field and a reactive rune the tours/tooltips read. Same module-singleton
// rune idiom as theme/state.svelte.ts and i18n/state.svelte.ts. No localStorage
// mirror: there's no FOUC concern (tours aren't first-paint critical).

import { commands, type ExplanationLevel } from '$lib/ipc/bindings';

// Initialised at startup directly from the settings `+page.svelte` already
// fetches (it sets `explanationState.level` inline, avoiding a second IPC
// round-trip) — so there is intentionally no separate init function here.
export const explanationState = $state<{ level: ExplanationLevel }>({ level: 'basic' });

/** Set the level: update the rune instantly (live UI), then persist via a
 *  read-modify-write of the whole `general` object (the theme/language path).
 *  A failed write still leaves the rune reflecting the choice for the session. */
export async function setExplanationLevel(level: ExplanationLevel): Promise<void> {
  explanationState.level = level;
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') return;
  const next = { ...get.data.general, explanation_level: level };
  await commands.appSettingsSetGeneral(next);
}
