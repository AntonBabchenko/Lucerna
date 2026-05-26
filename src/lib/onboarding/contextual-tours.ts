// src/lib/onboarding/contextual-tours.ts
//
// Per-surface one-shot tours (Manage / Logs / Modpacks / Worlds).
// Each runs once on first open of its host surface, then sets a
// localStorage key so it never reappears. Independent of the main
// onboarding tour in state.svelte.ts.

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
  worlds: 'v1',
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
    title: 'Display options',
    body: 'Wrap lines breaks long classpath / stack traces. Collapse stack hides repetitive Java frames behind a clickable row. The Show checkboxes hide entire severity levels — handy when chasing a single WARN through thousands of INFO lines.',
    targetSelector: '[data-tour-ctx="logs-toolbar"]',
    anchor: 'below',
  },
  {
    title: 'Search with navigation',
    body: 'Type to search the open file. Counter shows N of M matches; Enter / Shift+Enter step through them, Esc clears. Active match is highlighted in blue.',
    targetSelector: '[data-tour-ctx="logs-search"]',
    anchor: 'below',
  },
  {
    title: 'Share to mclo.gs',
    body: "Uploads the visible log to mclo.gs — the Minecraft community's de-facto paste service — and gives you a sharable link. Strips user paths, session tokens, and LAN IPs before upload so you can drop the link in Discord without leaking personal info.",
    targetSelector: '[data-tour-ctx="logs-share"]',
    anchor: 'below',
  },
  {
    title: 'Crash reports get a structured view',
    body: "When you open a crash-*.txt file, FTlauncher splits it into collapsible sections (Head, Affected level, System Details, Mods). Toggle Raw view in the toolbar if you'd rather grep the plain text.",
    targetSelector: '[data-tour-ctx="logs-sidebar"]',
    anchor: 'right',
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

export const WORLDS_STEPS: ReadonlyArray<TourStep> = [
  {
    title: 'Your singleplayer worlds',
    body: "Every world this instance has saved. Click a row to open actions: backup, restore, or delete. The badge with a 📦 shows how many backups you've taken.",
    targetSelector: '[data-tour-ctx="worlds-list"]',
    anchor: 'below',
  },
  {
    title: 'Back up before risky changes',
    body: "Back up now zips the world into FTlauncher's backup folder. Use it before installing a new mod, switching loader, or testing builds — restoring is one click away.",
    targetSelector: '[data-tour-ctx="worlds-list"]',
    anchor: 'below',
  },
  {
    title: 'Restore replaces or duplicates',
    body: 'View backups… opens the backup list per world. Restore offers two modes: Replace (swap the live world for the backup, the current state becomes its own auto-backup first) or As copy (extract alongside, keeping both).',
    targetSelector: '[data-tour-ctx="worlds-list"]',
    anchor: 'below',
  },
  {
    title: 'Open saves folder',
    body: 'Jumps to the saves/ folder in your file manager for manual operations (moving a world to another instance, copying out for a server upload, etc.).',
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
