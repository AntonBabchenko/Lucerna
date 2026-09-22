// Explanation detail level runtime — bridges the persisted GeneralSettings
// field and a reactive rune the tours/tooltips read. Same module-singleton
// rune idiom as theme/state.svelte.ts and i18n/state.svelte.ts. No localStorage
// mirror: there's no FOUC concern (tours aren't first-paint critical).

import type { ExplanationLevel } from '$lib/ipc/bindings';
import { patchGeneral } from '$lib/settings/app-settings.svelte';

// Initialised at startup directly from the settings `+page.svelte` already
// fetches (it sets `explanationState.level` inline, avoiding a second IPC
// round-trip) — so there is intentionally no separate init function here.
export const explanationState = $state<{ level: ExplanationLevel }>({ level: 'basic' });

/** Set the level: update the rune instantly (live UI), then persist through
 *  the one settings contract. When the patch is refused or lost, roll the rune
 *  back — but only while it still shows THIS pick: a newer pick may have
 *  landed meanwhile (the theme / language guard). */
export async function setExplanationLevel(level: ExplanationLevel): Promise<void> {
  const prev = explanationState.level;
  explanationState.level = level;
  const r = await patchGeneral({ explanation_level: level });
  if (!r.ok && explanationState.level === level) explanationState.level = prev;
}
