// src/lib/onboarding/contextual-tours.ts
//
// Three per-surface one-shot tours (Manage / Logs / Modpacks).
// Each runs once on first open of its host surface, then sets a
// localStorage key so it never reappears. Independent of the main
// onboarding tour in state.svelte.ts.

import type { TourStep } from './steps';

export type ContextualTourId = 'manage' | 'logs' | 'modpacks';

const STORAGE_KEY_PREFIX = 'ftl.tour.';
const STORAGE_KEY_SUFFIX = '.done';

export function storageKey(id: ContextualTourId): string {
  return `${STORAGE_KEY_PREFIX}${id}${STORAGE_KEY_SUFFIX}`;
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
    title: 'Your instances',
    body: 'Each row is one instance — click to edit it, or hit "+ New instance" below to add one.',
    targetSelector: '[data-tour-ctx="manage-list"]',
    anchor: 'right',
  },
  {
    title: 'Edit and save',
    body: 'Name, MC version, loader, memory, and JVM args live here. Done saves and closes.',
    targetSelector: '[data-tour-ctx="manage-form"]',
    anchor: 'right',
  },
  {
    title: 'Open folder, delete',
    body: "Open folder jumps to the instance's directory on disk. Delete wipes the instance and all its files — confirmation required.",
    targetSelector: '[data-tour-ctx="manage-actions"]',
    anchor: 'below',
  },
];

export const LOGS_STEPS: ReadonlyArray<TourStep> = [
  {
    title: 'Three log sources',
    body: "Game logs are Minecraft's output. Crash reports only appear when the game crashes. Launcher logs are FTlauncher's own.",
    targetSelector: '[data-tour-ctx="logs-sidebar"]',
    anchor: 'right',
  },
  {
    title: 'Huge logs',
    body: 'Read cap limits how many bytes the launcher loads — useful for multi-hundred-MB game logs. Reload re-scans the folder.',
    targetSelector: '[data-tour-ctx="logs-cap"]',
    anchor: 'below',
  },
  {
    title: 'Find inside a file',
    body: 'Search highlights every match in the open file. Case-insensitive.',
    targetSelector: '[data-tour-ctx="logs-search"]',
    anchor: 'below',
  },
];

export const MODPACKS_STEPS: ReadonlyArray<TourStep> = [
  {
    title: 'Browse vs Imported',
    body: "Browse searches Modrinth + CurseForge. Imported lists packs you've already installed — each is its own instance.",
    targetSelector: '[data-tour-ctx="modpacks-tabs"]',
    anchor: 'below',
  },
  {
    title: 'Drag and drop',
    body: 'Drop a .mrpack or CurseForge .zip anywhere on this view to import. Each import becomes a new instance — your existing ones stay untouched.',
    targetSelector: '[data-tour-ctx="modpacks-dropzone"]',
    anchor: 'below',
  },
  {
    title: 'Filter by MC + loader',
    body: 'Pick a Minecraft version or loader to narrow results. Empty = all versions. Click "Clear filters" to reset.',
    targetSelector: '[data-tour-ctx="modpacks-filters"]',
    anchor: 'below',
  },
];

export const STEPS_BY_ID: Record<ContextualTourId, ReadonlyArray<TourStep>> = {
  manage: MANAGE_STEPS,
  logs: LOGS_STEPS,
  modpacks: MODPACKS_STEPS,
};
