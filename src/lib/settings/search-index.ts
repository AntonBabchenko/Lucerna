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
  /**
   * The i18n keys the panel renders inside this anchor's SettingsField — its
   * heading, label, button — so the words printed on the page are searchable
   * without repeating them in the keywords, and so the search label can be
   * checked against them (every word of the label is a word the page shows).
   */
  visibleKeys: readonly TranslationKey[];
}

/**
 * Anchors where moving keyboard focus on jump is safe: text fields the user
 * came to type into. A toggle, select or section never takes focus (on a
 * slider a focus grab turns the next arrow key into a silent edit). The
 * CurseForge key field is one — the banners say "add a key" — and its
 * `ApiKeyField` marks the input with `data-flash-focus`, which `fieldFlash`
 * prefers over the first focusable and waits for if it is still disabled.
 */
const FOCUSABLE_ANCHORS: readonly SettingsAnchor[] = ['integrations.curseforgeKey'];

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
    visibleKeys: [
      'settings.general.appearance.theme',
      'settings.general.appearance.themeSystem',
      'settings.general.appearance.themeLight',
      'settings.general.appearance.themeDark',
    ],
  },
  'appearance.language': {
    anchor: 'appearance.language',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.language',
    keywordsKey: 'settings.search.keywords.appearance.language',
    visibleKeys: ['settings.general.appearance.language'],
  },
  'appearance.rainbowIcons': {
    anchor: 'appearance.rainbowIcons',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.rainbowIcons',
    keywordsKey: 'settings.search.keywords.appearance.rainbowIcons',
    visibleKeys: ['settings.general.appearance.rainbowIcons'],
  },
  'appearance.iconZoom': {
    anchor: 'appearance.iconZoom',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.iconZoom',
    keywordsKey: 'settings.search.keywords.appearance.iconZoom',
    visibleKeys: ['settings.general.appearance.iconZoom'],
  },
  'appearance.sidebarButtons': {
    anchor: 'appearance.sidebarButtons',
    tab: 'appearance',
    labelKey: 'settings.search.labels.appearance.sidebarButtons',
    keywordsKey: 'settings.search.keywords.appearance.sidebarButtons',
    visibleKeys: ['settings.general.appearance.sidebarButtons.title'],
  },
  'game.tray': {
    anchor: 'game.tray',
    tab: 'game',
    labelKey: 'settings.search.labels.game.tray',
    keywordsKey: 'settings.search.keywords.game.tray',
    visibleKeys: ['settings.general.playing.title', 'settings.general.playing.trayLabel'],
  },
  'game.serverPing': {
    anchor: 'game.serverPing',
    tab: 'game',
    labelKey: 'settings.search.labels.game.serverPing',
    keywordsKey: 'settings.search.keywords.game.serverPing',
    visibleKeys: ['settings.general.serverPing.title', 'settings.general.serverPing.label'],
  },
  'game.gpu': {
    anchor: 'game.gpu',
    tab: 'game',
    labelKey: 'settings.search.labels.game.gpu',
    keywordsKey: 'settings.search.keywords.game.gpu',
    visibleKeys: ['settings.general.gpu.title', 'settings.general.gpu.label'],
  },
  'integrations.curseforgeKey': {
    anchor: 'integrations.curseforgeKey',
    tab: 'integrations',
    labelKey: 'settings.search.labels.integrations.curseforgeKey',
    keywordsKey: 'settings.search.keywords.integrations.curseforgeKey',
    visibleKeys: ['settings.curseforge.title', 'settings.curseforge.inputLabelNew'],
  },
  'integrations.aiTranslation': {
    anchor: 'integrations.aiTranslation',
    tab: 'integrations',
    labelKey: 'settings.search.labels.integrations.aiTranslation',
    keywordsKey: 'settings.search.keywords.integrations.aiTranslation',
    visibleKeys: ['settings.aiTranslation.title'],
  },
  'storage.cache': {
    anchor: 'storage.cache',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.cache',
    keywordsKey: 'settings.search.keywords.storage.cache',
    visibleKeys: ['settings.storage.cacheTitle', 'settings.storage.clearBtn'],
  },
  'storage.logRetention': {
    anchor: 'storage.logRetention',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.logRetention',
    keywordsKey: 'settings.search.keywords.storage.logRetention',
    visibleKeys: [
      'settings.general.logRetention.title',
      'settings.general.logRetention.enableLabel',
    ],
  },
  'storage.modMetadataCache': {
    anchor: 'storage.modMetadataCache',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.modMetadataCache',
    keywordsKey: 'settings.search.keywords.storage.modMetadataCache',
    visibleKeys: [
      'settings.general.modMetadataCache.title',
      'settings.general.modMetadataCache.ttlLabel',
    ],
  },
  'storage.dataLocation': {
    anchor: 'storage.dataLocation',
    tab: 'storage',
    labelKey: 'settings.search.labels.storage.dataLocation',
    keywordsKey: 'settings.search.keywords.storage.dataLocation',
    visibleKeys: ['settings.storage.dataLocation.heading'],
  },
  'updates.startupCheck': {
    anchor: 'updates.startupCheck',
    tab: 'updates',
    labelKey: 'settings.search.labels.updates.startupCheck',
    keywordsKey: 'settings.search.keywords.updates.startupCheck',
    visibleKeys: ['settings.general.updates.startupLabel'],
  },
  'updates.changelog': {
    anchor: 'updates.changelog',
    tab: 'updates',
    labelKey: 'settings.search.labels.updates.changelog',
    keywordsKey: 'settings.search.keywords.updates.changelog',
    visibleKeys: ['settings.changelog.title'],
  },
  'help.tipsLevel': {
    anchor: 'help.tipsLevel',
    tab: 'help',
    labelKey: 'settings.search.labels.help.tipsLevel',
    keywordsKey: 'settings.search.keywords.help.tipsLevel',
    visibleKeys: ['settings.general.tips.title', 'settings.general.tips.levelLabel'],
  },
  'help.replayTours': {
    anchor: 'help.replayTours',
    tab: 'help',
    labelKey: 'settings.search.labels.help.replayTours',
    keywordsKey: 'settings.search.keywords.help.replayTours',
    visibleKeys: ['settings.general.onboarding.title', 'settings.general.onboarding.replayBtn'],
  },
  'about.repo': {
    anchor: 'about.repo',
    tab: 'about',
    labelKey: 'settings.search.labels.about.repo',
    keywordsKey: 'settings.search.keywords.about.repo',
    visibleKeys: ['settings.about.viewOnGitHub'],
  },
};

/** Registry values in display order. Consumed by the search field. */
export const SETTINGS_ENTRIES: SettingsSearchEntry[] = Object.values(SETTINGS_SEARCH);
