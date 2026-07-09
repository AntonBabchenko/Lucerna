// src/lib/onboarding/contextual-tours.ts
//
// Per-surface one-shot tours (Manage / Logs / Modpacks / Worlds).
// Each runs once on first open of its host surface, then sets a
// localStorage key so it never reappears. Independent of the main
// onboarding tour in state.svelte.ts.
//
// titleKey / bodyKey are TranslationKey references resolved at render
// time via $t(step.titleKey). The actual text lives in
// src/lib/i18n/locales/{en,ru}.json under onboarding.contextual.*.

import type { TourStep } from './steps';

export type ContextualTourId =
  | 'manage'
  | 'logs'
  | 'modpacks'
  | 'worlds'
  | 'servers'
  | 'serverManage'
  | 'addons';

const STORAGE_KEY_PREFIX = 'ftl.tour.';
const STORAGE_KEY_SUFFIX = '.done';

// Per-tour content version. Bump when the steps change materially so
// users who dismissed the previous version see the new one once. The
// old key (without the version suffix or with a lower one) stays in
// localStorage as harmless cruft.
const TOUR_VERSION: Record<ContextualTourId, string> = {
  manage: 'v2', // bumped 2026-06-23 — added the Verify/repair step (was an orphan anchor)
  logs: 'v4', // bumped 2026-06-23 — added diagnosis + read-cap steps
  modpacks: 'v1',
  worlds: 'v3', // bumped 2026-06-23 — added the import-a-world step
  servers: 'v1', // added 2026-06-23 — Servers list tour
  serverManage: 'v1', // added 2026-06-23 — server detail/manage tour
  addons: 'v1', // added 2026-06-23 — Add-ons tab layout tour
};

export function storageKey(id: ContextualTourId): string {
  return `${STORAGE_KEY_PREFIX}${id}.${TOUR_VERSION[id]}${STORAGE_KEY_SUFFIX}`;
}

export function hasSeen(id: ContextualTourId): boolean {
  try {
    return localStorage.getItem(storageKey(id)) === '1';
  } catch {
    return false;
  }
}

export function markSeen(id: ContextualTourId): void {
  try {
    localStorage.setItem(storageKey(id), '1');
  } catch {
    /* private-mode etc.; tour will fire again next time, acceptable */
  }
}

export const MANAGE_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.manage.yourInstances.title',
    bodyKey: 'onboarding.contextual.manage.yourInstances.body',
    targetSelector: '[data-tour-ctx="manage-list"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.contextual.manage.editAndSave.title',
    bodyKey: 'onboarding.contextual.manage.editAndSave.body',
    targetSelector: '[data-tour-ctx="manage-form"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.contextual.manage.openFolderDelete.title',
    bodyKey: 'onboarding.contextual.manage.openFolderDelete.body',
    targetSelector: '[data-tour-ctx="manage-actions"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.manage.verifyRepair.title',
    bodyKey: 'onboarding.contextual.manage.verifyRepair.body',
    targetSelector: '[data-tour-ctx="manage-integrity"]',
    anchor: 'below',
  },
];

export const LOGS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.logs.threeLogSources.title',
    bodyKey: 'onboarding.contextual.logs.threeLogSources.body',
    targetSelector: '[data-tour-ctx="logs-sidebar"]',
    anchor: 'right',
  },
  {
    // Anchors the diagnosis banner's own root. The banner renders only when the
    // latest log carries a known problem, so when it is absent the spotlight
    // falls back to a centred popover and the copy still teaches the concept.
    titleKey: 'onboarding.contextual.logs.diagnosis.title',
    bodyKey: 'onboarding.contextual.logs.diagnosis.body',
    targetSelector: '[data-tour-ctx="logs-diagnosis"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.logs.displayOptions.title',
    bodyKey: 'onboarding.contextual.logs.displayOptions.body',
    targetSelector: '[data-tour-ctx="logs-toolbar"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.logs.readCap.title',
    bodyKey: 'onboarding.contextual.logs.readCap.body',
    targetSelector: '[data-tour-ctx="logs-cap"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.logs.searchNavigation.title',
    bodyKey: 'onboarding.contextual.logs.searchNavigation.body',
    targetSelector: '[data-tour-ctx="logs-search"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.logs.shareMcloGs.title',
    bodyKey: 'onboarding.contextual.logs.shareMcloGs.body',
    targetSelector: '[data-tour-ctx="logs-overflow"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.logs.crashStructuredView.title',
    bodyKey: 'onboarding.contextual.logs.crashStructuredView.body',
    targetSelector: '[data-tour-ctx="logs-sidebar"]',
    anchor: 'right',
  },
];

export const MODPACKS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.modpacks.browseVsImported.title',
    bodyKey: 'onboarding.contextual.modpacks.browseVsImported.body',
    targetSelector: '[data-tour-ctx="modpacks-tabs"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.modpacks.dragAndDrop.title',
    bodyKey: 'onboarding.contextual.modpacks.dragAndDrop.body',
    targetSelector: '[data-tour-ctx="modpacks-dropzone"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.modpacks.searchSortFilter.title',
    bodyKey: 'onboarding.contextual.modpacks.searchSortFilter.body',
    targetSelector: '[data-tour-ctx="modpacks-filters"]',
    anchor: 'below',
  },
];

export const WORLDS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.worlds.worldsWithBackups.title',
    bodyKey: 'onboarding.contextual.worlds.worldsWithBackups.body',
    targetSelector: '[data-tour-ctx="worlds-list"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.worlds.importWorlds.title',
    bodyKey: 'onboarding.contextual.worlds.importWorlds.body',
    targetSelector: '[data-tour-ctx="worlds-import"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.worlds.openSavesFolder.title',
    bodyKey: 'onboarding.contextual.worlds.openSavesFolder.body',
    targetSelector: '[data-tour-ctx="worlds-open-folder"]',
    anchor: 'right',
  },
];

// Servers list view (ServersView). Fires on first open of the Servers modal,
// when the list is typically EMPTY — so every step anchors a stable element
// (Create button, the list wrapper, the always-present LAN hint), never a
// server row, which may not exist yet.
export const SERVERS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.servers.create.title',
    bodyKey: 'onboarding.contextual.servers.create.body',
    targetSelector: '[data-tour-ctx="servers-create"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.servers.list.title',
    bodyKey: 'onboarding.contextual.servers.list.body',
    targetSelector: '[data-tour-ctx="servers-list"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.servers.lanHint.title',
    bodyKey: 'onboarding.contextual.servers.lanHint.body',
    targetSelector: '[data-tour-ctx="servers-lan"]',
    anchor: 'below',
  },
];

// Server detail view (ServerManageView). The 8 sub-tabs render content only
// while active, so steps anchor the always-present tab BUTTONS (and the header
// actions), not tab bodies. The crash-diagnosis banner is empty until a crash,
// so it has no step of its own — it is described in the header-actions step.
export const SERVER_MANAGE_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.serverManage.headerActions.title',
    bodyKey: 'onboarding.contextual.serverManage.headerActions.body',
    targetSelector: '[data-tour-ctx="server-header-actions"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.console.title',
    bodyKey: 'onboarding.contextual.serverManage.console.body',
    targetSelector: '[data-tour-ctx="server-tab-console"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.mods.title',
    bodyKey: 'onboarding.contextual.serverManage.mods.body',
    targetSelector: '[data-tour-ctx="server-tab-mods"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.connect.title',
    bodyKey: 'onboarding.contextual.serverManage.connect.body',
    targetSelector: '[data-tour-ctx="server-tab-connect"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.hosting.title',
    bodyKey: 'onboarding.contextual.serverManage.hosting.body',
    targetSelector: '[data-tour-ctx="server-tab-hosting"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.toInstance.title',
    bodyKey: 'onboarding.contextual.serverManage.toInstance.body',
    targetSelector: '[data-tour-ctx="server-to-instance"]',
    anchor: 'below',
  },
];

// Add-ons tab (AddonsTab). Fires on first open. Defaults (kind='mod',
// view='browse') keep all three anchors present, and each anchors a stable
// layout element — the conditional preflight panel is taught by per-button
// tooltips instead, since it only appears when there are dependency violations.
export const ADDONS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.addons.kindSwitch.title',
    bodyKey: 'onboarding.contextual.addons.kindSwitch.body',
    targetSelector: '[data-tour-ctx="addons-kind-switch"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.addons.subtabs.title',
    bodyKey: 'onboarding.contextual.addons.subtabs.body',
    targetSelector: '[data-tour-ctx="addons-subtabs"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.addons.dropzone.title',
    bodyKey: 'onboarding.contextual.addons.dropzone.body',
    targetSelector: '[data-tour-ctx="addons-dropzone"]',
    anchor: 'below',
  },
];

export const STEPS_BY_ID: Record<ContextualTourId, ReadonlyArray<TourStep>> = {
  manage: MANAGE_STEPS,
  logs: LOGS_STEPS,
  modpacks: MODPACKS_STEPS,
  worlds: WORLDS_STEPS,
  servers: SERVERS_STEPS,
  serverManage: SERVER_MANAGE_STEPS,
  addons: ADDONS_STEPS,
};

// Single source for iterating every contextual tour — derived from
// STEPS_BY_ID so a newly added tour can never be silently skipped by a
// reset/audit that forgot to list it.
export const ALL_CONTEXTUAL_TOUR_IDS = Object.keys(STEPS_BY_ID) as ContextualTourId[];

// Clear the "seen" flag for every contextual tour so each re-fires on the
// next visit to its surface. The Settings "Replay onboarding" action calls
// this; without it, replay restarts only the main tour and leaves the
// per-surface tours (Manage / Logs / Modpacks / Worlds / Servers / server
// detail) suppressed forever.
export function resetAllContextualTours(): void {
  for (const id of ALL_CONTEXTUAL_TOUR_IDS) {
    try {
      localStorage.removeItem(storageKey(id));
    } catch {
      /* private-mode etc.; best-effort, the tour simply won't reset */
    }
  }
}
