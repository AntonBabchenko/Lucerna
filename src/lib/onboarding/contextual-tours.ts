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

export type ContextualTourId = 'manage' | 'logs' | 'modpacks' | 'worlds';

const STORAGE_KEY_PREFIX = 'ftl.tour.';
const STORAGE_KEY_SUFFIX = '.done';

// Per-tour content version. Bump when the steps change materially so
// users who dismissed the previous version see the new one once. The
// old key (without the version suffix or with a lower one) stays in
// localStorage as harmless cruft.
const TOUR_VERSION: Record<ContextualTourId, string> = {
  manage: 'v1',
  logs: 'v2', // bumped 2026-05-26 — log viewer v2 features added
  modpacks: 'v1',
  worlds: 'v2', // bumped 2026-05-26 — collapsed 4 steps into 2, dropped per-action stubs
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
];

export const LOGS_STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.contextual.logs.threeLogSources.title',
    bodyKey: 'onboarding.contextual.logs.threeLogSources.body',
    targetSelector: '[data-tour-ctx="logs-sidebar"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.contextual.logs.displayOptions.title',
    bodyKey: 'onboarding.contextual.logs.displayOptions.body',
    targetSelector: '[data-tour-ctx="logs-toolbar"]',
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
    targetSelector: '[data-tour-ctx="logs-share"]',
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
    titleKey: 'onboarding.contextual.worlds.openSavesFolder.title',
    bodyKey: 'onboarding.contextual.worlds.openSavesFolder.body',
    targetSelector: '[data-tour-ctx="worlds-open-folder"]',
    anchor: 'right',
  },
];

export const STEPS_BY_ID: Record<ContextualTourId, ReadonlyArray<TourStep>> = {
  manage: MANAGE_STEPS,
  logs: LOGS_STEPS,
  modpacks: MODPACKS_STEPS,
  worlds: WORLDS_STEPS,
};

// Single source for iterating every contextual tour — derived from
// STEPS_BY_ID so a newly added tour can never be silently skipped by a
// reset/audit that forgot to list it.
export const ALL_CONTEXTUAL_TOUR_IDS = Object.keys(STEPS_BY_ID) as ContextualTourId[];

// Clear the "seen" flag for every contextual tour so each re-fires on the
// next visit to its surface. The Settings "Replay onboarding" action calls
// this; without it, replay restarts only the main tour and leaves the
// per-surface tours (Manage / Logs / Modpacks / Worlds) suppressed forever.
export function resetAllContextualTours(): void {
  for (const id of ALL_CONTEXTUAL_TOUR_IDS) {
    try {
      localStorage.removeItem(storageKey(id));
    } catch {
      /* private-mode etc.; best-effort, the tour simply won't reset */
    }
  }
}
