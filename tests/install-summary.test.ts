import { describe, expect, it } from 'vitest';
import type { ModVersion, TaskDetail } from '$lib/ipc/bindings';
import type { DepItem, OptionalItem } from '$lib/mods/dep-prompt';
import { buildInstalledDepLines, skippedFiles } from '$lib/mods/install-summary';

// ---------------------------------------------------------------------------
// Minimal stubs — only the fields touched by buildInstalledDepLines.
// ---------------------------------------------------------------------------

function mv(source: 'modrinth' | 'curseforge', projectId: string, versionId: string): ModVersion {
  return {
    source,
    project_id: projectId,
    version_id: versionId,
    name: `${projectId}-${versionId}`,
    version_number: '1.0',
    mc_versions: [],
    loaders: [],
    primary_file: { filename: '', url: '', sha1: null, size: 0, distribution_allowed: true },
    deps: [],
    published_at: null,
  };
}

function dep(name: string, version: ModVersion): DepItem {
  return { version, projectName: name, projectSource: version.source };
}

function optional(name: string, version: ModVersion, requires: DepItem[] = []): OptionalItem {
  return { ...dep(name, version), requires };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('buildInstalledDepLines', () => {
  it('returns an empty array when there are no required deps and no chosen optionals', () => {
    const lines = buildInstalledDepLines({ required: [], optional: [] }, []);
    expect(lines).toEqual([]);
  });

  it('lists required dep names in order when there are no chosen optionals', () => {
    const req1 = dep('Mod A', mv('modrinth', 'a', 'va'));
    const req2 = dep('Mod B', mv('modrinth', 'b', 'vb'));
    const lines = buildInstalledDepLines({ required: [req1, req2], optional: [] }, []);
    expect(lines).toEqual(['Mod A', 'Mod B']);
  });

  it('appends a chosen optional and its sub-requires after required deps', () => {
    const reqDep = dep('Required', mv('modrinth', 'req', 'vr'));
    const subReq = dep('Sub Require', mv('modrinth', 'sub', 'vs'));
    const opt = optional('Optional', mv('modrinth', 'opt', 'vo'), [subReq]);

    const lines = buildInstalledDepLines({ required: [reqDep], optional: [opt] }, [opt.version]);
    expect(lines).toEqual(['Required', 'Optional', 'Sub Require']);
  });

  it('deduplicates entries that appear in both required and optional sub-requires', () => {
    const sharedVersion = mv('modrinth', 'shared', 'vs');
    const reqDep = dep('Shared Mod', sharedVersion);
    const subReq = dep('Shared Mod', sharedVersion); // same source:project_id
    const opt = optional('Optional', mv('modrinth', 'opt', 'vo'), [subReq]);

    const lines = buildInstalledDepLines({ required: [reqDep], optional: [opt] }, [opt.version]);
    // 'Shared Mod' must appear only once (first-seen wins — from required).
    expect(lines).toEqual(['Shared Mod', 'Optional']);
  });

  it('deduplicates two optionals that share a sub-require', () => {
    const sharedSub = dep('Shared Sub', mv('modrinth', 'shared', 'vs'));
    const opt1 = optional('Optional 1', mv('modrinth', 'o1', 'vo1'), [sharedSub]);
    const opt2 = optional('Optional 2', mv('modrinth', 'o2', 'vo2'), [sharedSub]);

    const lines = buildInstalledDepLines({ required: [], optional: [opt1, opt2] }, [
      opt1.version,
      opt2.version,
    ]);
    expect(lines).toEqual(['Optional 1', 'Shared Sub', 'Optional 2']);
  });

  it('skips a chosenOptional entry with no matching prompt.optional entry', () => {
    const unknownVersion = mv('modrinth', 'unknown', 'vx');
    const lines = buildInstalledDepLines({ required: [], optional: [] }, [unknownVersion]);
    expect(lines).toEqual([]);
  });

  it('deduplicates across modrinth and curseforge using source:project_id key', () => {
    // Two different versions of the same Modrinth project — same source:project_id.
    const v1 = mv('modrinth', 'proj', 'v1');
    const v2 = mv('modrinth', 'proj', 'v2'); // same project_id, different version_id
    const req = dep('Proj', v1);
    const subR = dep('Proj', v2);
    const opt = optional('Optional', mv('modrinth', 'opt', 'vo'), [subR]);

    const lines = buildInstalledDepLines({ required: [req], optional: [opt] }, [opt.version]);
    // 'Proj' appears first via required, sub-req is deduped.
    expect(lines).toEqual(['Proj', 'Optional']);
  });
});

// Spec 2026-10-09 same-mod-by-id D3: the install may leave a planned dependency out because the
// profile already has its mod (another project, the same mod-id). The toast must not name it as
// installed — it says it was already there.
describe('a dependency the install left out', () => {
  const withFile = (v: ModVersion, filename: string): ModVersion => ({
    ...v,
    primary_file: { ...v.primary_file, filename },
  });

  it('reads as already installed, not installed', () => {
    const balm = dep('Balm', withFile(mv('curseforge', '531761', 'b'), 'balm-21.0.66.jar'));
    const lib = dep('Lib', withFile(mv('curseforge', 'lib', 'l'), 'lib.jar'));
    const lines = buildInstalledDepLines(
      { required: [balm, lib], optional: [] },
      [],
      new Set(['balm-21.0.66.jar']),
      (name) => `${name} was already there`,
    );
    expect(lines).toEqual(['Balm was already there', 'Lib']);
  });

  it('is read off the report: its skipped rows, by file name', () => {
    const row = (install_path: string, outcome: TaskDetail['outcome']): TaskDetail => ({
      name: install_path,
      install_path,
      origin: 'curseforge',
      host: null,
      bytes: null,
      sha1: null,
      outcome,
    });
    const files = skippedFiles([
      row('mods/waystones.jar', { kind: 'unchanged' }),
      row('mods/balm-21.0.66.jar', { kind: 'skipped', reason: 'Balm is already installed' }),
    ]);
    expect([...files]).toEqual(['balm-21.0.66.jar']);
  });
});
