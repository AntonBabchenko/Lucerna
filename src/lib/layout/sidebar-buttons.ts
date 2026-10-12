// Single source of truth for the user-hideable sidebar buttons. Pure module
// (no runes) so both the sidebar and the settings panel import the same list
// and IDs. IDs are stable, opaque strings persisted in
// GeneralSettings.hidden_sidebar_buttons; the display order here is the order
// the checkboxes appear in Settings → Appearance. A retired id still listed in
// someone's saved settings ('manage', 2026-10-12) is inert: visibility is a
// membership test on the ids below.

import type { TranslationKey } from '$lib/i18n/keys.generated';

export type SidebarButtonId =
  | 'skin'
  | 'account_actions'
  | 'mods'
  | 'quick_join'
  | 'new_instance'
  | 'browse_modpacks'
  | 'import_launcher'
  | 'gallery'
  | 'logs';

export interface SidebarButtonDescriptor {
  id: SidebarButtonId;
  labelKey: TranslationKey;
}

export const SIDEBAR_BUTTONS: readonly SidebarButtonDescriptor[] = [
  { id: 'skin', labelKey: 'settings.general.appearance.sidebarButtons.skin' },
  { id: 'account_actions', labelKey: 'settings.general.appearance.sidebarButtons.accountActions' },
  { id: 'mods', labelKey: 'settings.general.appearance.sidebarButtons.mods' },
  { id: 'quick_join', labelKey: 'settings.general.appearance.sidebarButtons.quickJoin' },
  { id: 'new_instance', labelKey: 'settings.general.appearance.sidebarButtons.newInstance' },
  { id: 'browse_modpacks', labelKey: 'settings.general.appearance.sidebarButtons.browseModpacks' },
  { id: 'import_launcher', labelKey: 'settings.general.appearance.sidebarButtons.importLauncher' },
  { id: 'gallery', labelKey: 'settings.general.appearance.sidebarButtons.gallery' },
  { id: 'logs', labelKey: 'settings.general.appearance.sidebarButtons.logs' },
];
