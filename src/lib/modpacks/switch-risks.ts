// Pure logic for switching an installed modpack to another published version.
// No IPC and no Svelte: everything here is a function of data the switch dialog
// already holds, so it is unit-testable and cannot drift from what the user sees.

import type {
  InstanceWithStatus,
  ModpackVersionBump,
  ModpackVersionEntry,
} from '$lib/ipc/bindings';

/** Where the target version sits relative to the installed one. */
export type SwitchDirection = 'upgrade' | 'downgrade' | 'reinstall' | 'unknown';

export type SwitchRisk =
  | { kind: 'mc-change'; from: string; to: string }
  | { kind: 'downgrade' }
  | { kind: 'loader-change'; from: string; to: string }
  | { kind: 'customizations'; userAdded: number; manual: number }
  | { kind: 'bundled-overrides' };

export type SwitchRiskInput = {
  direction: SwitchDirection;
  /** From `modpack_compute_update` — the backend's authoritative MC/loader delta. */
  versionBump: ModpackVersionBump | null;
  /** Mods added via the Mod browser after import (not in `pack_origin`). */
  userAdded: number;
  /** Jars the user dropped into `mods/` by hand (no recorded source). */
  manual: number;
  /** The pack bundles `overrides/` content — `pack_origin` files with an empty url. */
  hasBundledFiles: boolean;
};

/**
 * Newest-first by `date_published` — the same ordering rule `latest_newer` uses
 * on the backend, so the picker cannot disagree with the update banner about
 * which version is latest. Returns a new array; the input is not mutated.
 *
 * Version *numbers* are deliberately never compared: pack version strings are
 * free-form and ordering them would be a guess.
 */
export function sortVersionsNewestFirst(versions: ModpackVersionEntry[]): ModpackVersionEntry[] {
  return [...versions].sort((a, b) => b.date_published.localeCompare(a.date_published));
}

/**
 * Direction by position in the newest-first list. An installed version absent
 * from the list (delisted, or a drag-drop import with no provenance) yields
 * `unknown` — we cannot prove the target is not older, so callers must not
 * treat it as safe.
 */
export function switchDirection(
  versions: ModpackVersionEntry[],
  installedVersionId: string | null,
  targetVersionId: string,
): SwitchDirection {
  if (installedVersionId === null) return 'unknown';
  if (installedVersionId === targetVersionId) return 'reinstall';
  const sorted = sortVersionsNewestFirst(versions);
  const installedAt = sorted.findIndex((v) => v.id === installedVersionId);
  const targetAt = sorted.findIndex((v) => v.id === targetVersionId);
  if (installedAt === -1 || targetAt === -1) return 'unknown';
  return targetAt < installedAt ? 'upgrade' : 'downgrade';
}

/**
 * The warnings to show before applying a switch, most-consequential first.
 *
 * `mc-change` states the change but not which Minecraft version is older.
 * Naming a direction would need a release-ordered manifest (a network fetch with
 * its own failure states), and parsing "1.20.1" is a guess that snapshots and
 * the 2026 `26.x` scheme break. The hazard is symmetric anyway — a world is at
 * risk whenever the Minecraft version changes.
 */
export function assessSwitchRisks(input: SwitchRiskInput): SwitchRisk[] {
  const risks: SwitchRisk[] = [];
  const bump = input.versionBump;

  if (bump !== null && bump.old_game_version !== bump.new_game_version) {
    risks.push({ kind: 'mc-change', from: bump.old_game_version, to: bump.new_game_version });
  }
  if (input.direction === 'downgrade' || input.direction === 'unknown') {
    risks.push({ kind: 'downgrade' });
  }
  if (bump !== null && bump.old_loader_version !== bump.new_loader_version) {
    risks.push({
      kind: 'loader-change',
      from: bump.old_loader_version ?? '—',
      to: bump.new_loader_version ?? '—',
    });
  }
  if (input.userAdded > 0 || input.manual > 0) {
    risks.push({ kind: 'customizations', userAdded: input.userAdded, manual: input.manual });
  }
  if (input.hasBundledFiles) {
    risks.push({ kind: 'bundled-overrides' });
  }
  return risks;
}

/**
 * The base version for a pack changelog request. It must be the source's
 * version ID — `PackOrigin.version` is a free-form version NUMBER and never
 * matches `changelog_window`'s id list, which silently collapsed the cumulative
 * pack changelog to target-only.
 */
export function packChangelogBase(
  inst: Pick<InstanceWithStatus, 'mrpack_version_id'>,
): string | null {
  return inst.mrpack_version_id;
}

export type SwitchChangelogRequest = {
  target: string;
  base: string | null;
  titleKey: 'modpacks.switch.changelogGained' | 'modpacks.switch.changelogLost';
};

/**
 * Which version range to show in the changelog, and under which framing.
 *
 * `changelog_window` returns base-exclusive → target-inclusive and degrades to
 * target-only when base is not older than target. So direction is expressed
 * purely by which id goes in which slot:
 *
 * - upgrade   → base = installed, target = chosen  → the versions being applied.
 * - downgrade → base = chosen, target = installed  → the versions being rolled
 *   back past, i.e. what the user loses. Without the swap a downgrade would fall
 *   into the target-only branch and render the old release notes under a
 *   "what's new" framing, reading as if nothing were being lost.
 * - reinstall / unknown → that version's own notes only.
 */
export function switchChangelogRequest(
  direction: SwitchDirection,
  installedVersionId: string | null,
  targetVersionId: string,
): SwitchChangelogRequest {
  if (installedVersionId !== null && direction === 'downgrade') {
    return {
      target: installedVersionId,
      base: targetVersionId,
      titleKey: 'modpacks.switch.changelogLost',
    };
  }
  if (installedVersionId !== null && direction === 'upgrade') {
    return {
      target: targetVersionId,
      base: installedVersionId,
      titleKey: 'modpacks.switch.changelogGained',
    };
  }
  return { target: targetVersionId, base: null, titleKey: 'modpacks.switch.changelogGained' };
}
