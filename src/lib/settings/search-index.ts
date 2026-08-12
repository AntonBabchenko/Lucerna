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

/** The registry. Record<SettingsAnchor, …> ⇒ every anchor MUST appear exactly
 *  once (compile error otherwise). Insertion order = display order (grouped by
 *  section). Each labelKey/keywordsKey follows `settings.search.{labels,keywords}.<anchor>`. */
export const SETTINGS_SEARCH: Record<SettingsAnchor, SettingsSearchEntry> = {
  'appearance.theme': {
    anchor: 'appearance.theme',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.theme',
    keywordsKey: 'settings.search.keywords.appearance.theme',
  },
  'appearance.language': {
    anchor: 'appearance.language',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.language',
    keywordsKey: 'settings.search.keywords.appearance.language',
  },
  'appearance.rainbowIcons': {
    anchor: 'appearance.rainbowIcons',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.rainbowIcons',
    keywordsKey: 'settings.search.keywords.appearance.rainbowIcons',
  },
  'appearance.iconZoom': {
    anchor: 'appearance.iconZoom',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.iconZoom',
    keywordsKey: 'settings.search.keywords.appearance.iconZoom',
  },
  'appearance.sidebarButtons': {
    anchor: 'appearance.sidebarButtons',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.sidebarButtons',
    keywordsKey: 'settings.search.keywords.appearance.sidebarButtons',
  },
  'game.tray': {
    anchor: 'game.tray',
    tab: 'game',
    labelKey: 'settings.search.labels.game.tray',
    keywordsKey: 'settings.search.keywords.game.tray',
  },
  'game.serverPing': {
    anchor: 'game.serverPing',
    tab: 'game',
    labelKey: 'settings.search.labels.game.serverPing',
    keywordsKey: 'settings.search.keywords.game.serverPing',
  },
  'game.gpu': {
    anchor: 'game.gpu',
    tab: 'game',
    labelKey: 'settings.search.labels.game.gpu',
    keywordsKey: 'settings.search.keywords.game.gpu',
  },
  'integrations.curseforgeKey': {
    anchor: 'integrations.curseforgeKey',
    tab: 'integrations',
    labelKey: 'settings.search.labels.integrations.curseforgeKey',
    keywordsKey: 'settings.search.keywords.integrations.curseforgeKey',
  },
  'integrations.urlScheme': {
    anchor: 'integrations.urlScheme',
    tab: 'integrations',
    labelKey: 'settings.search.labels.integrations.urlScheme',
    keywordsKey: 'settings.search.keywords.integrations.urlScheme',
  },
  'integrations.aiTranslation': {
    anchor: 'integrations.aiTranslation',
    tab: 'integrations',
    labelKey: 'settings.search.labels.integrations.aiTranslation',
    keywordsKey: 'settings.search.keywords.integrations.aiTranslation',
  },
  'storage.cache': {
    anchor: 'storage.cache',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.cache',
    keywordsKey: 'settings.search.keywords.storage.cache',
  },
  'storage.logRetention': {
    anchor: 'storage.logRetention',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.logRetention',
    keywordsKey: 'settings.search.keywords.storage.logRetention',
  },
  'storage.modMetadataCache': {
    anchor: 'storage.modMetadataCache',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.modMetadataCache',
    keywordsKey: 'settings.search.keywords.storage.modMetadataCache',
  },
  'storage.dataLocation': {
    anchor: 'storage.dataLocation',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.dataLocation',
    keywordsKey: 'settings.search.keywords.storage.dataLocation',
  },
  'updates.startupCheck': {
    anchor: 'updates.startupCheck',
    tab: 'updates',
    labelKey: 'settings.search.labels.updates.startupCheck',
    keywordsKey: 'settings.search.keywords.updates.startupCheck',
  },
  'updates.changelog': {
    anchor: 'updates.changelog',
    tab: 'updates',
    labelKey: 'settings.search.labels.updates.changelog',
    keywordsKey: 'settings.search.keywords.updates.changelog',
  },
  'help.tipsLevel': {
    anchor: 'help.tipsLevel',
    tab: 'help',
    labelKey: 'settings.search.labels.help.tipsLevel',
    keywordsKey: 'settings.search.keywords.help.tipsLevel',
  },
  'help.replayTours': {
    anchor: 'help.replayTours',
    tab: 'help',
    labelKey: 'settings.search.labels.help.replayTours',
    keywordsKey: 'settings.search.keywords.help.replayTours',
  },
  'about.repo': {
    anchor: 'about.repo',
    tab: 'about',
    labelKey: 'settings.search.labels.about.repo',
    keywordsKey: 'settings.search.keywords.about.repo',
  },
};

/** Registry values in display order. Consumed by the search field. */
export const SETTINGS_ENTRIES: SettingsSearchEntry[] = Object.values(SETTINGS_SEARCH);
