// Single source of truth for the user-hideable sidebar buttons. Pure module
// (no runes) so both the sidebar and the settings panel import the same list
// and IDs. IDs are stable, opaque strings persisted in
// GeneralSettings.hidden_sidebar_buttons; the display order here is the order
// the checkboxes appear in Settings → Appearance.

import type { TranslationKey } from '$lib/i18n/keys.generated';

export type SidebarButtonId =
  | 'account_actions'
  | 'manage'
  | 'mods'
  | 'quick_join'
  | 'browse_modpacks'
  | 'import_launcher'
  | 'gallery'
  | 'logs';

export interface SidebarButtonDescriptor {
  id: SidebarButtonId;
  labelKey: TranslationKey;
}

export const SIDEBAR_BUTTONS: readonly SidebarButtonDescriptor[] = [
  { id: 'account_actions', labelKey: 'settings.general.appearance.sidebarButtons.accountActions' },
  { id: 'manage', labelKey: 'settings.general.appearance.sidebarButtons.manage' },
  { id: 'mods', labelKey: 'settings.general.appearance.sidebarButtons.mods' },
  { id: 'quick_join', labelKey: 'settings.general.appearance.sidebarButtons.quickJoin' },
  { id: 'browse_modpacks', labelKey: 'settings.general.appearance.sidebarButtons.browseModpacks' },
  { id: 'import_launcher', labelKey: 'settings.general.appearance.sidebarButtons.importLauncher' },
  { id: 'gallery', labelKey: 'settings.general.appearance.sidebarButtons.gallery' },
  { id: 'logs', labelKey: 'settings.general.appearance.sidebarButtons.logs' },
];
