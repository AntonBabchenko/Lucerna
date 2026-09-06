import { describe, expect, it } from 'vitest';
import type {
  InstanceWithStatus,
  LeftReason,
  MigrationPlan,
  UnknownReason,
} from '$lib/ipc/bindings';
import {
  datapackSummary,
  migrateDisabledKey,
  splitTargets,
  verdictKey,
} from '$lib/worlds/migrate-plan-text';

// Pure text/gating helpers behind MigrateWorldDialog (spec §6, §7). Every
// verdict and every Unknown reason has its own key — a reason without a
// sentence would render a raw key, which the Record in the module makes a
// compile error and this file pins at runtime.

function makePlan(over: Partial<MigrationPlan> = {}): MigrationPlan {
  return {
    world_version_name: null,
    verdict: { kind: 'same' },
    source_loader: 'vanilla',
    target_loader: 'vanilla',
    mods_missing_in_target: 0,
    datapacks: [],
    datapacks_folders: 0,
    ...over,
  };
}

function instance(id: string, mc_version: string): InstanceWithStatus {
  return {
    id,
    name: id,
    mc_version,
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
  };
}

describe('verdictKey', () => {
  it('renders nothing for Same (§6 says "nothing")', () => {
    expect(verdictKey({ kind: 'same' })).toBeNull();
  });

  it('maps the two ordered verdicts', () => {
    expect(verdictKey({ kind: 'will_upgrade' })).toBe('worlds.migrate.verdict.willUpgrade');
    expect(verdictKey({ kind: 'world_is_newer' })).toBe('worlds.migrate.verdict.worldIsNewer');
  });

  const UNKNOWN: ReadonlyArray<[reason: UnknownReason, key: string]> = [
    ['no_level_dat', 'worlds.migrate.verdict.unknown.noLevelDat'],
    ['unreadable', 'worlds.migrate.verdict.unknown.unreadable'],
    ['not_recorded', 'worlds.migrate.verdict.unknown.notRecorded'],
    ['target_version_unset', 'worlds.migrate.verdict.unknown.targetVersionUnset'],
    ['target_not_installed', 'worlds.migrate.verdict.unknown.targetNotInstalled'],
    ['target_not_recorded', 'worlds.migrate.verdict.unknown.targetNotRecorded'],
  ];

  it.each(UNKNOWN)('maps Unknown { %s } to its own sentence', (reason, key) => {
    expect(verdictKey({ kind: 'unknown', reason })).toBe(key);
  });
});

describe('datapackSummary', () => {
  it('counts linked, kept-as-copy and folder packs from the plan', () => {
    const plan = makePlan({
      datapacks_folders: 2,
      datapacks: [
        { filename: 'a.zip', predicted: { kind: 'linked' } },
        { filename: 'b.zip', predicted: { kind: 'linked' } },
        { filename: 'c.zip', predicted: { kind: 'linked' } },
        { filename: 'd.zip', predicted: { kind: 'adopted' } },
        {
          filename: 'e.zip',
          predicted: { kind: 'left_as_copy', reason: { kind: 'name_held_by_different_pack' } },
        },
      ],
    });
    expect(datapackSummary(plan)).toEqual({
      total: 5,
      inTarget: 3,
      keptNameTaken: 1,
      keptNotAdded: 0,
      folders: 2,
    });
  });

  // Only `NameHeldByDifferentPack` licenses the "its name is in use" sentence.
  // The other five reasons mean the pack stays a plain copy for a cause the
  // dialog has NOT established, so they land in the quieter bucket rather than
  // borrowing a claim about a pack nobody compared it with.
  const OTHER_REASONS: readonly LeftReason[] = [
    { kind: 'not_a_datapack', reason: 'not_a_zip' },
    { kind: 'too_large' },
    { kind: 'link_failed' },
    { kind: 'unreadable' },
    { kind: 'io' },
  ];

  it.each(OTHER_REASONS)('counts left-as-copy ($kind) as "not added"', (reason) => {
    const plan = makePlan({
      datapacks: [{ filename: 'a.zip', predicted: { kind: 'left_as_copy', reason } }],
    });
    expect(datapackSummary(plan)).toEqual({
      total: 1,
      inTarget: 0,
      keptNameTaken: 0,
      keptNotAdded: 1,
      folders: 0,
    });
  });

  it('is all zeros for a world without datapacks', () => {
    expect(datapackSummary(makePlan())).toEqual({
      total: 0,
      inTarget: 0,
      keptNameTaken: 0,
      keptNotAdded: 0,
      folders: 0,
    });
  });
});

describe('splitTargets', () => {
  it('drops the source and separates instances with no version set', () => {
    const src = instance('src', '1.20.1');
    const a = instance('a', '1.21.1');
    const b = instance('b', '');
    const split = splitTargets([src, a, b], 'src');
    expect(split.candidates.map((i) => i.id)).toEqual(['a']);
    expect(split.excludedNoVersion.map((i) => i.id)).toEqual(['b']);
  });

  it('never lists the source, even when it has no version itself', () => {
    const src = instance('src', '');
    expect(splitTargets([src], 'src')).toEqual({ candidates: [], excludedNoVersion: [] });
  });
});

describe('migrateDisabledKey', () => {
  const open = { fellBack: false, sourceBusy: false, hasTarget: true, planning: false };

  it('returns null when nothing blocks the migration', () => {
    expect(migrateDisabledKey(open)).toBeNull();
  });

  it('puts the data-root fallback first', () => {
    expect(migrateDisabledKey({ ...open, fellBack: true, sourceBusy: true })).toBe(
      'page.dataRootFallback.createDisabledReason',
    );
  });

  it('then a task active for the source', () => {
    expect(migrateDisabledKey({ ...open, sourceBusy: true, hasTarget: false })).toBe(
      'worlds.migrate.disabledBusy',
    );
  });

  it('then a missing target', () => {
    expect(migrateDisabledKey({ ...open, hasTarget: false, planning: true })).toBe(
      'worlds.migrate.disabledNoTarget',
    );
  });

  it('then a plan still in flight', () => {
    expect(migrateDisabledKey({ ...open, planning: true })).toBe('worlds.migrate.disabledPlanning');
  });
});
