// src/lib/onboarding/contextual-tours.ts
//
// Per-surface one-shot tours — see ContextualTourId for the full list.
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
  | 'addons'
  | 'serverAddons'
  | 'l10n'
  | 'overview';

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
  worlds: 'v4', // bumped 2026-08-12 — the row now opens the Backups | Datapacks dialog
  servers: 'v2', // bumped 2026-07-10 — the modal became the servers mode; steps re-anchored
  serverManage: 'v3', // bumped 2026-07-31 — step 1 re-anchored to the sidebar Start/Stop
  addons: 'v2', // bumped 2026-08-12 — 4th kind (datapacks) + new library step; subtabs copy corrected
  serverAddons: 'v1', // added 2026-08-12 — server Add-ons tab: kinds + drag-and-drop install
  l10n: 'v1', // added 2026-08-12 — mod-localization surface: coverage, find-string, actions
  overview: 'v1', // added 2026-08-12 — points at the localization row on the instance Overview
};

// FNV-1a 32-bit over the JSON of the FULL step objects (kind and disclaimerKey
// participate — a chooser/disclaimer edit is a material step change too).
// Recorded values live right here so any steps edit forces a diff beside the
// version map and the "does this need a bump?" question gets asked (the #294
// miss). tests/tour-fingerprint.test.ts recomputes and prints the new value on
// mismatch.
export function fingerprintSteps(steps: ReadonlyArray<TourStep>): string {
  const json = JSON.stringify(steps);
  let h = 0x811c9dc5;
  for (let i = 0; i < json.length; i++) {
    h ^= json.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
}

export const STEPS_FINGERPRINT: Record<ContextualTourId, string> = {
  manage: '797d4f7f',
  logs: '08499e92',
  modpacks: '31786820',
  worlds: '17315acc',
  servers: '88a55cc7',
  serverManage: '189b9426',
  addons: '68ef3e7c',
  serverAddons: '0b8f24b5',
  l10n: 'e10b491b',
  overview: '34bc85b6',
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
    targetSelector: '[data-tour-ctx="logs-sidebar"]',
    anchor: 'right',
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

// Servers mode, first entry (ServersPanel mounts this tour only in the empty
// state, so a server row never needs anchoring). Anchors: the sidebar create
// button and the mode switcher, both always present in servers mode.
export const SERVERS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.servers.create.title',
    bodyKey: 'onboarding.contextual.servers.create.body',
    targetSelector: '[data-tour-ctx="servers-create"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.contextual.servers.modeSwitch.title',
    bodyKey: 'onboarding.contextual.servers.modeSwitch.body',
    targetSelector: '[data-tour-ctx="servers-mode-switch"]',
    anchor: 'right',
  },
];

// Server detail view (ServersPanel). The 5 tabs render content only
// while active, so steps anchor the always-present tab BUTTONS (and the
// sidebar Start/Stop), not tab bodies. The crash-diagnosis banner is empty
// until a crash, so it has no step of its own — it is described in the
// start/stop step. Settings and Backups carry no step: the tour teaches the
// surfaces whose grouping changed in v2, not every tab.
export const SERVER_MANAGE_STEPS: ReadonlyArray<TourStep> = [
  {
    // Start/Stop lives in the SIDEBAR (the mirror of the client Play button),
    // not in the panel header — anchoring this step to the header actions made
    // it spotlight the "create client instance" button instead.
    titleKey: 'onboarding.contextual.serverManage.startStop.title',
    bodyKey: 'onboarding.contextual.serverManage.startStop.body',
    targetSelector: '[data-tour-ctx="server-start-stop"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.overview.title',
    bodyKey: 'onboarding.contextual.serverManage.overview.body',
    targetSelector: '[data-tour-ctx="server-tab-overview"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverManage.addons.title',
    bodyKey: 'onboarding.contextual.serverManage.addons.body',
    targetSelector: '[data-tour-ctx="server-tab-addons"]',
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
    // Rides the same anchor as kindSwitch (precedent: three logs steps anchor
    // logs-sidebar) — the datapack tab body only exists once that kind is
    // selected, and the switch is the stable element the concept hangs off.
    titleKey: 'onboarding.contextual.addons.datapackLibrary.title',
    bodyKey: 'onboarding.contextual.addons.datapackLibrary.body',
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

// Server Add-ons tab (ServerAddonsTab). Mounted gated on the datapack-gate
// answer having LANDED and the kind list being non-empty — the optimistic
// default would otherwise burn this tour on a pre-1.13 vanilla server's first
// visit, where the kind switch it anchors renders with nothing to switch to.
export const SERVER_ADDONS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.serverAddons.kindSwitch.title',
    bodyKey: 'onboarding.contextual.serverAddons.kindSwitch.body',
    targetSelector: '[data-tour-ctx="server-addons-kind-switch"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.serverAddons.dropzone.title',
    bodyKey: 'onboarding.contextual.serverAddons.dropzone.body',
    targetSelector: '[data-tour-ctx="server-addons-dropzone"]',
    anchor: 'below',
  },
];

// Mod-localization surface. Mounted gated on the coverage fetch resolving
// non-empty, so a zero-mod instance (reachable via Manage → Translations)
// never burns the tour over the empty state — its first step anchors the
// coverage list, which is exactly what an empty result does not render.
export const L10N_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.l10n.coverage.title',
    bodyKey: 'onboarding.contextual.l10n.coverage.body',
    targetSelector: '[data-tour-ctx="l10n-coverage"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.contextual.l10n.search.title',
    bodyKey: 'onboarding.contextual.l10n.search.body',
    targetSelector: '[data-tour-ctx="l10n-search"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.l10n.actions.title',
    bodyKey: 'onboarding.contextual.l10n.actions.body',
    targetSelector: '[data-tour-ctx="l10n-actions"]',
    anchor: 'below',
  },
];

// Instance Overview — one step pointing at the localization row, the entry
// point to the l10n surface. The host mount is REACTIVE on `!tourState.active`
// (unlike every other host, which mounts once): Overview is the default tab,
// so it is already mounted at startup racing initOnboarding, and a one-shot
// mount check would either fire under the main tour or be lost entirely.
export const OVERVIEW_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.overview.l10nRow.title',
    bodyKey: 'onboarding.contextual.overview.l10nRow.body',
    targetSelector: '[data-tour-ctx="overview-l10n"]',
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
  serverAddons: SERVER_ADDONS_STEPS,
  l10n: L10N_STEPS,
  overview: OVERVIEW_STEPS,
};

// Single source for iterating every contextual tour — derived from
// STEPS_BY_ID so a newly added tour can never be silently skipped by a
// reset/audit that forgot to list it.
export const ALL_CONTEXTUAL_TOUR_IDS = Object.keys(STEPS_BY_ID) as ContextualTourId[];

// Clear the "seen" flag for every contextual tour so each re-fires on the
// next visit to its surface. The Settings "Replay onboarding" action calls
// this; without it, replay restarts only the main tour and leaves every
// per-surface tour (see ContextualTourId) suppressed forever.
export function resetAllContextualTours(): void {
  for (const id of ALL_CONTEXTUAL_TOUR_IDS) {
    try {
      localStorage.removeItem(storageKey(id));
    } catch {
      /* private-mode etc.; best-effort, the tour simply won't reset */
    }
  }
}
