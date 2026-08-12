// The searchable-settings registry: the single source of truth for what the
// Settings search can find and where each hit lives. Declared as a
// Record<SettingsAnchor, …> so a missing or stray entry is a COMPILE error
// (mirrors the make-omission-a-compile-error pattern). The SETTINGS_SEARCH
// literal and SETTINGS_ENTRIES are added in a later step once the i18n keys
// exist.
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { SettingsTab } from './state.svelte';

/** Stable id for one searchable control. `<tab>.<control>`. */
export type SettingsAnchor =
  | 'appearance.theme'
  | 'appearance.language'
  | 'appearance.rainbowIcons'
  | 'appearance.iconZoom'
  | 'appearance.sidebarButtons'
  | 'game.tray'
  | 'game.serverPing'
  | 'game.gpu'
  | 'integrations.curseforgeKey'
  | 'integrations.urlScheme'
  | 'integrations.aiTranslation'
  | 'storage.cache'
  | 'storage.logRetention'
  | 'storage.modMetadataCache'
  | 'storage.dataLocation'
  | 'updates.startupCheck'
  | 'updates.changelog'
  | 'help.tipsLevel'
  | 'help.replayTours'
  | 'about.repo';

export interface SettingsSearchEntry {
  anchor: SettingsAnchor;
  tab: SettingsTab;
  labelKey: TranslationKey;
  keywordsKey: TranslationKey;
}

/**
 * Anchors where moving keyboard focus on jump is safe. Empty in v1: every
 * current settings control is a toggle, select, button or section — none is a
 * text field the user would immediately type into, and the API-key inputs are
 * not the first focusable inside their wrapped section. The seam exists (rather
 * than a hardcoded `false`) so a future text-input setting can opt in, exactly
 * as `manage-focus.ts`'s `shouldFocusField` does.
 */
const FOCUSABLE_ANCHORS: readonly SettingsAnchor[] = [];

export function shouldFocusAnchor(anchor: SettingsAnchor): boolean {
  return FOCUSABLE_ANCHORS.includes(anchor);
}
