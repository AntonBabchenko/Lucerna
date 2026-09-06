// Pure text/gating helpers for MigrateWorldDialog — one source of truth for
// the verdict sentence key, the datapack summary counts, the target split and
// the confirm gate, so the dialog and its tests cannot drift (the
// `dataRootCreateDisabledKey` / `quickPlayDisabledKey` shape).
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type {
  InstanceWithStatus,
  LeftReason,
  MigrationPlan,
  UnknownReason,
  VersionVerdict,
} from '$lib/ipc/bindings';
import { datapackRejectionKey } from '$lib/ipc/format-error';
import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

/** Record, not a switch: a new `UnknownReason` without a sentence must not compile. */
const UNKNOWN_REASON_KEY: Record<UnknownReason, TranslationKey> = {
  no_level_dat: 'worlds.migrate.verdict.unknown.noLevelDat',
  unreadable: 'worlds.migrate.verdict.unknown.unreadable',
  not_recorded: 'worlds.migrate.verdict.unknown.notRecorded',
  target_version_unset: 'worlds.migrate.verdict.unknown.targetVersionUnset',
  target_not_installed: 'worlds.migrate.verdict.unknown.targetNotInstalled',
  target_not_recorded: 'worlds.migrate.verdict.unknown.targetNotRecorded',
};

const ORDERED_VERDICT_KEY: Record<'will_upgrade' | 'world_is_newer', TranslationKey> = {
  will_upgrade: 'worlds.migrate.verdict.willUpgrade',
  world_is_newer: 'worlds.migrate.verdict.worldIsNewer',
};

/**
 * The i18n key of the spec §6 verdict sentence, or `null` for `Same` (the
 * table says "nothing"). Every key takes `{name}` (the world's recorded
 * version name, or the `versionNameUnknown` phrase) and `{target}` (the
 * target instance's Minecraft version); an unused argument is harmless.
 */
export function verdictKey(v: VersionVerdict): TranslationKey | null {
  if (v.kind === 'same') return null;
  if (v.kind === 'unknown') return UNKNOWN_REASON_KEY[v.reason];
  return ORDERED_VERDICT_KEY[v.kind];
}

/**
 * Which sentence a predicted `left_as_copy` earns. `NameHeldByDifferentPack`
 * is the one cause the dialog can state exactly; the other five mean only that
 * the pack will not join the target library, and saying "its name is in use by
 * a different pack" about a file that could not be read — or about a stat that
 * failed — would be a confident claim about something nobody checked. Record,
 * not a switch: a seventh `LeftReason` without a bucket must not compile.
 */
const KEPT_BUCKET: Record<LeftReason['kind'], 'nameTaken' | 'notAdded'> = {
  name_held_by_different_pack: 'nameTaken',
  not_a_datapack: 'notAdded',
  too_large: 'notAdded',
  link_failed: 'notAdded',
  unreadable: 'notAdded',
  io: 'notAdded',
};

/**
 * The outcome-time counterpart of `KEPT_BUCKET`. Once the migration has run
 * the real cause is known per pack, so the completion toast names it instead
 * of the two plan-time buckets above — those exist only because `predict_one`
 * cannot tell the five non-name causes apart before the copy happens. Record,
 * not a switch, for the same reason: a seventh `LeftReason` without a sentence
 * must not compile. `NotADatapack` is excluded because it carries a typed
 * `DatapackRejection` of its own — see `leftReasonKey`.
 */
const LEFT_REASON_KEY: Record<Exclude<LeftReason['kind'], 'not_a_datapack'>, TranslationKey> = {
  name_held_by_different_pack: 'worlds.migrate.leftReason.nameHeldByDifferentPack',
  too_large: 'worlds.migrate.leftReason.tooLarge',
  link_failed: 'worlds.migrate.leftReason.linkFailed',
  unreadable: 'worlds.migrate.leftReason.unreadable',
  io: 'worlds.migrate.leftReason.io',
};

/**
 * Why a datapack stayed a plain copy in the migrated world instead of being
 * linked into the target library (spec §5 steps 4–5), as the sentence the
 * completion toast reads out after "… not linked to the target library:".
 *
 * `NotADatapack` is the one variant the backend qualifies: it carries the same
 * typed `DatapackRejection` the library's own "this file is not a datapack"
 * error does, and "it is a resource pack" is something the user can act on.
 * Reusing `datapackRejectionKey` keeps that sentence identical wherever the
 * rejection surfaces, instead of paraphrasing it into an umbrella here (spec
 * §7 "Completion").
 */
export function leftReasonKey(r: LeftReason): TranslationKey {
  if (r.kind === 'not_a_datapack') return datapackRejectionKey(r.reason);
  return LEFT_REASON_KEY[r.kind];
}

export interface DatapackSummary {
  /** Every `.zip` pack the plan looked at. */
  total: number;
  /** Predicted `linked`: the target library already holds the same bytes. */
  inTarget: number;
  /** Predicted `left_as_copy` because a different pack holds the name. */
  keptNameTaken: number;
  /** Predicted `left_as_copy` for any other reason — the dialog then says only
   *  that these will not join the target library, never why. */
  keptNotAdded: number;
  /** Folder packs, copied as they are (spec §5). */
  folders: number;
}

/** Counts for the "N datapacks — …" lines. What is left after `inTarget` and
 *  the two kept buckets is what will be adopted into the target library:
 *  plan-time `predict_one` returns only `Adopted`, `Linked` and `LeftAsCopy`
 *  (`CopiedNotLinked` is a runtime fallback of the real copy), so nothing else
 *  can hide in that remainder. */
export function datapackSummary(plan: MigrationPlan): DatapackSummary {
  const kept = plan.datapacks.flatMap((d) =>
    d.predicted.kind === 'left_as_copy' ? [KEPT_BUCKET[d.predicted.reason.kind]] : [],
  );
  return {
    total: plan.datapacks.length,
    inTarget: plan.datapacks.filter((d) => d.predicted.kind === 'linked').length,
    keptNameTaken: kept.filter((b) => b === 'nameTaken').length,
    keptNotAdded: kept.filter((b) => b === 'notAdded').length,
    folders: plan.datapacks_folders,
  };
}

export interface TargetSplit {
  /** Instances the picker offers, in the caller's order. */
  candidates: InstanceWithStatus[];
  /** Excluded because `mc_version` is empty (fresh-install state) — named
   *  under the picker so the exclusion is stated, not silent (spec §7). */
  excludedNoVersion: InstanceWithStatus[];
}

/** The source itself is never a target; an instance with no Minecraft version
 *  set cannot be planned against (`TargetVersionUnset`) and is listed as
 *  excluded instead of offered. Running instances stay in `candidates` — the
 *  dialog disables them per option from the live running state. */
export function splitTargets(instances: InstanceWithStatus[], sourceId: string): TargetSplit {
  const others = instances.filter((i) => i.id !== sourceId);
  return {
    candidates: others.filter((i) => i.mc_version !== ''),
    excludedNoVersion: others.filter((i) => i.mc_version === ''),
  };
}

export interface MigrateGateState {
  /** The configured data root is unavailable (`dataLocation.fellBack`). */
  fellBack: boolean;
  /** The picker has something to offer (`splitTargets().candidates`). */
  hasCandidates: boolean;
  /** `taskFor({ instanceId: source })` is non-null. */
  sourceBusy: boolean;
  /** A target instance is chosen. */
  hasTarget: boolean;
  /** `world_migration_plan` for the chosen target is still in flight. */
  planning: boolean;
}

/**
 * Why the confirm button is disabled, or `null` when the migration may start.
 * Priority: data root fallen back → nothing to migrate to → source busy → no
 * target → plan in flight. A failed plan and every §6 verdict leave the button
 * enabled (D3).
 *
 * An empty picker outranks the busy and no-target reasons because it is the
 * only one the user cannot act on: "Choose a target instance first." above an
 * empty picker is an instruction with nothing to follow it with. The sentence
 * lives here rather than in the dialog so both places that say it — the note
 * replacing the picker and this reason — cannot drift apart.
 */
export function migrateDisabledKey(s: MigrateGateState): TranslationKey | null {
  const fallen = dataRootCreateDisabledKey(s.fellBack);
  if (fallen !== null) return fallen;
  if (!s.hasCandidates) return 'worlds.migrate.noTargets';
  if (s.sourceBusy) return 'worlds.migrate.disabledBusy';
  if (!s.hasTarget) return 'worlds.migrate.disabledNoTarget';
  if (s.planning) return 'worlds.migrate.disabledPlanning';
  return null;
}
